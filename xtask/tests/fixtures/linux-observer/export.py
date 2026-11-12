# Fixed oracle export. Mutable files belong to one container invocation.
import hashlib
import json
import os
import re
import stat

CHUNK = 131072
MANIFEST_WIRE = 65536
CHUNK_WIRE = 786432


def name_ok(name):
    if name in {"stdout", "stderr", "status.json"}:
        return True
    match = re.fullmatch(r"trace\.([1-9][0-9]{0,9})", name)
    return bool(match and int(match[1]) <= 4294967295)


def identity(value):
    return {"device": value.st_dev, "inode": value.st_ino, "mode": value.st_mode, "uid": value.st_uid, "links": value.st_nlink, "size": value.st_size, "mtime_ns": value.st_mtime_ns, "ctime_ns": value.st_ctime_ns}


def directory(root, uid):
    fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    value = os.fstat(fd)
    if not stat.S_ISDIR(value.st_mode) or value.st_uid != uid:
        os.close(fd)
        raise ValueError("export root ownership differs")
    return fd


def opened(fd, name, uid):
    if not name_ok(name):
        raise ValueError("unsafe export name")
    value = os.stat(name, dir_fd=fd, follow_symlinks=False)
    if not stat.S_ISREG(value.st_mode) or value.st_uid != uid or value.st_nlink != 1:
        raise ValueError("unsafe export entry")
    file = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
    current = os.fstat(file)
    if identity(value) != identity(current):
        os.close(file)
        raise ValueError("export entry changed while opening")
    return file, identity(current)


def encoded(value, bound):
    output = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    if len(output.encode()) + 1 > bound:
        raise ValueError("export wire bound exceeded")
    return output


def manifest(root, uid):
    fd = directory(root, uid)
    files = []
    issues = []
    trace_count = 0
    trace_bytes = 0
    try:
        names = []
        with os.scandir(fd) as entries:
            for entry in entries:
                if len(names) == 64:
                    issues.append("entry-limit")
                    break
                names.append(entry.name)
        for name in sorted(names):
            if not name_ok(name):
                issues.append("unexpected-entry:" + hashlib.sha256(os.fsencode(name)).hexdigest())
                continue
            file = None
            try:
                file, stamp = opened(fd, name, uid)
                limit = 8388608 if name.startswith("trace.") else 1048576
                if stamp["size"] < 0 or stamp["size"] > limit:
                    raise ValueError("file-size-limit")
                if name.startswith("trace."):
                    trace_count += 1
                    trace_bytes += stamp["size"]
                    if trace_count > 32 or trace_bytes > 8388608:
                        raise ValueError("trace-aggregate-limit")
                hasher = hashlib.sha256()
                total = 0
                while True:
                    data = os.read(file, 65536)
                    if not data:
                        break
                    total += len(data)
                    if total > stamp["size"]:
                        raise ValueError("file-size-changed")
                    hasher.update(data)
                if total != stamp["size"] or identity(os.fstat(file)) != stamp:
                    raise ValueError("file-identity-changed")
                files.append({"name": name, "identity": stamp, "sha256": hasher.hexdigest()})
            except (OSError, ValueError):
                issues.append("unavailable-or-unsafe-entry:" + hashlib.sha256(name.encode()).hexdigest())
            finally:
                if file is not None:
                    os.close(file)
        accepted = {file["name"] for file in files}
        if not {"stdout", "stderr", "status.json"}.issubset(accepted) or not any(name.startswith("trace.") for name in accepted):
            issues.append("missing-required-files")
        result = {"version": 1, "files": files, "issues": sorted(set(issues))}
        encoded(result, MANIFEST_WIRE)
        return result
    finally:
        os.close(fd)


def chunk(root, uid, name, offset, length, stamp, before_read=None):
    if type(offset) is not int or type(length) is not int or offset < 0 or not 0 < length <= CHUNK:
        raise ValueError("invalid export range")
    if not isinstance(stamp, dict) or any(type(value) is not int or value < 0 for value in stamp.values()) or type(stamp.get("size")) is not int or offset + length > stamp["size"]:
        raise ValueError("range exceeds manifest")
    fd = directory(root, uid)
    file = None
    try:
        file, actual = opened(fd, name, uid)
        if actual["size"] > (8388608 if name.startswith("trace.") else 1048576):
            raise ValueError("export file bound exceeded")
        if actual != stamp:
            raise ValueError("manifest identity changed")
        if before_read is not None:
            before_read()
        data = os.pread(file, length, offset)
        if len(data) != length or identity(os.fstat(file)) != stamp:
            raise ValueError("export read changed or short")
        result = {"version": 1, "name": name, "offset": offset, "length": length, "identity": stamp, "sha256": hashlib.sha256(data).hexdigest(), "data": list(data)}
        encoded(result, CHUNK_WIRE)
        return result
    finally:
        if file is not None:
            os.close(file)
        os.close(fd)


def main(root, uid, args):
    if args == ["--export-manifest"]:
        result, limit = manifest(root, uid), MANIFEST_WIRE
    elif len(args) == 5 and args[0] == "--export-chunk":
        if not re.fullmatch(r"0|[1-9][0-9]*", args[2]) or not re.fullmatch(r"[1-9][0-9]*", args[3]):
            raise ValueError("noncanonical export range")
        result = chunk(root, uid, args[1], int(args[2]), int(args[3]), json.loads(args[4]))
        limit = CHUNK_WIRE
    else:
        raise ValueError("unsupported export command")
    print(encoded(result, limit))
