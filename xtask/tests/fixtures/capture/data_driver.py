"""Run one preregistered synthetic store API phase in a fresh process."""

import os
import sys

os.umask(0o022)

# This observer belongs only to the bootstrap child, before site can import providers.
_bootstrap = sys.argv[1:] == ["--runtime-bootstrap"]
_store_calls = []
_previous_profile = sys.getprofile()
_store_source = os.environ["CAREERBREW_CAPTURE_ROOT"] + "/src/careerbrew/store.py"


def _store_profile(frame, event, _argument):
    if event == "call" and frame.f_code.co_filename == _store_source and frame.f_code.co_name != "<module>":
        if len(_store_calls) >= 1024:
            raise RuntimeError("bootstrap call ledger exceeds bound")
        _store_calls.append({"ordinal": len(_store_calls) + 1, "function": frame.f_code.co_name})


if _bootstrap:
    sys.setprofile(_store_profile)

import site  # noqa: E402 (set the child umask before site imports)

site.main()

import copy  # noqa: E402 (common import-only bootstrap and phase dependency)
import datetime  # noqa: E402 (child umask and site setup precede imports)
import hashlib  # noqa: E402 (child umask and site setup precede imports)
import inspect  # noqa: E402 (child umask and site setup precede imports)
import json  # noqa: E402 (child umask and site setup precede imports)
import struct  # noqa: E402 (child umask and site setup precede imports)
from pathlib import Path  # noqa: E402 (child umask and site setup precede imports)

from careerbrew import backup as backup  # noqa: E402 (declare original lazy provider closure)
from careerbrew import store  # noqa: E402 (child umask and site setup precede imports)

FIXTURE_SHA256 = "6025c63de77ed2ed9fb6a69a95860a33b38b675840edbf187405918c23222fe2"


def decode(value):
    kind = value["kind"]
    if kind == "null":
        return None
    if kind == "bool":
        if type(value["value"]) is not bool:
            raise ValueError("invalid boolean")
        return value["value"]
    if kind == "int":
        result = int(value["decimal"])
        if str(result) != value["decimal"]:
            raise ValueError("noncanonical integer")
        return result
    if kind == "float64":
        return struct.unpack(">d", bytes.fromhex(value["bits"]))[0]
    if kind == "str" and value["encoding"] == "literal":
        return value["value"]
    if kind == "date":
        result = datetime.date.fromisoformat(value["iso"])
        if result.isoformat() != value["iso"]:
            raise ValueError("noncanonical date")
        return result
    if kind == "datetime":
        result = datetime.datetime.fromisoformat(value["iso"])
        if result.isoformat() != value["iso"]:
            raise ValueError("noncanonical datetime")
        return result
    if kind in ("list", "tuple"):
        result = [decode(item) for item in value["items"]]
        return tuple(result) if kind == "tuple" else result
    if kind == "map":
        result = {}
        for entry in value["entries"]:
            key = decode(entry["key"])
            if type(key) is not str or key in result:
                raise ValueError("invalid map key")
            result[key] = decode(entry["value"])
        return result
    raise ValueError("unsupported input node")


def encode(value):
    kind = type(value)
    if value is None:
        return {"kind": "null"}
    if kind is bool:
        return {"kind": "bool", "value": value}
    if kind is int:
        return {"kind": "int", "decimal": str(value)}
    if kind is float:
        return {"kind": "float64", "bits": struct.pack(">d", value).hex()}
    if kind is str:
        return {"kind": "str", "encoding": "literal", "value": value}
    if kind is datetime.datetime:
        return {"kind": "datetime", "iso": value.isoformat()}
    if kind is datetime.date:
        return {"kind": "date", "iso": value.isoformat()}
    if kind in (list, tuple):
        return {"kind": "list" if kind is list else "tuple", "items": [encode(item) for item in value]}
    if kind is dict:
        if not all(type(key) is str for key in value):
            raise ValueError("unsupported output map key")
        return {"kind": "map", "entries": [{"key": encode(key), "value": encode(value[key])} for key in sorted(value, key=lambda item: item.encode("utf-8"))]}
    raise ValueError("unsupported output type")


def digest_text(value):
    if type(value) is not str:
        raise ValueError("digest projection requires text")
    raw = value.encode("utf-8")
    return {"kind": "str", "encoding": "sha256", "utf8_bytes": str(len(raw)), "sha256": hashlib.sha256(raw).hexdigest()}


def actual_target(value, owned, expected):
    relative = expected.split("/")
    if any(part in ("", ".", "..") for part in relative):
        raise ValueError("invalid declared path")
    lexical = os.fspath(value)
    if lexical != str(owned) + "/" + expected:
        raise ValueError("actual target differs")
    current = owned
    for part in relative:
        current = current / part
        if current.is_symlink():
            raise ValueError("symlink target refused")
    return {"kind": "path", "root": "owned", "relative": expected}


def invoke(spec, arguments, target, owned, case, role):
    original = getattr(store, spec["function"].rsplit(".", 1)[1])
    function = original.__module__ + "." + original.__qualname__
    if function != spec["function"]:
        raise ValueError("provider entry differs")
    bound = inspect.signature(original).bind(target, **arguments)
    bound.apply_defaults()
    observed_target = actual_target(bound.arguments["path"], owned, spec["observed_target"]["relative"])
    bindings = {name: observed_target if name == "path" else encode(value) for name, value in bound.arguments.items()}
    wire = json.dumps(bindings, sort_keys=True, ensure_ascii=True, separators=(",", ":"), allow_nan=False).encode("utf-8")
    source = Path(original.__code__.co_filename)
    entry = {
        "sequence": spec["sequence"],
        "purpose": spec["purpose"],
        "function": function,
        "arguments_sha256": hashlib.sha256(wire).hexdigest(),
        "target": observed_target,
        "module_path": str(Path(store.__file__).resolve()),
        "loader_origin": str(Path(store.__spec__.origin).resolve()),
        "function_source": str(source),
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "started": True,
    }
    result = original(*bound.args, **bound.kwargs)
    use_digest = role == "reader" and (spec["purpose"] == "serialized-bytes" or (spec["purpose"] == "primary" and case in ("2", "8")))
    expected_projection = "str-sha256" if use_digest else "recursive-typed-literal"
    if spec["return_projection"] != expected_projection:
        raise ValueError("projection rule differs")
    entry["returned"] = digest_text(result) if use_digest else encode(result)
    entry["completed"] = True
    return entry


def diagnostic_sample(stage):
    allowed = {os.environ["CAREERBREW_CAPTURE_ROOT"] + "/xtask/tests/fixtures/capture/data_driver.py"}

    def path_fact(value):
        if value is None:
            return None, None
        text = str(value)
        if text not in allowed:
            return "untrusted-path-sha256:" + hashlib.sha256(text.encode()).hexdigest(), None
        path = Path(text)
        if path.is_symlink() or not path.is_file():
            return text, None
        return text, hashlib.sha256(path.read_bytes()).hexdigest()

    module = sys.modules.get("__main__")
    spec = globals().get("__spec__")
    module_spec = getattr(module, "__spec__", None)
    own_file, own_hash = path_fact(globals().get("__file__"))
    code_file, code_hash = path_fact(main.__code__.co_filename)
    module_file, module_hash = path_fact(getattr(module, "__file__", None))
    return {
        "stage": stage,
        "globals_file": own_file,
        "globals_file_sha256": own_hash,
        "globals_name": globals().get("__name__"),
        "globals_spec_name": getattr(spec, "name", None),
        "globals_spec_origin": path_fact(getattr(spec, "origin", None))[0],
        "main_code_filename": code_file,
        "main_code_sha256": code_hash,
        "sys_modules_main_present": "__main__" in sys.modules,
        "sys_modules_main_file": module_file,
        "sys_modules_main_file_sha256": module_hash,
        "sys_modules_main_name": getattr(module, "__name__", None),
        "sys_modules_main_spec_name": getattr(module_spec, "name", None),
        "sys_modules_main_spec_origin": path_fact(getattr(module_spec, "origin", None))[0],
    }


def bootstrap_main(entry):
    fault = os.environ.get("CAREERBREW_DATA_BOOTSTRAP_CONTROL", "none")
    if fault not in ("none", "store-call", "extra-write", "stdout-canary", "skip", "nonzero"):
        raise ValueError("bootstrap control differs")
    owned = Path(os.environ["CAREERBREW_DATA_OWNED"])
    if str(owned) != str(owned.resolve()):
        raise ValueError("bootstrap owned root differs")
    if fault == "skip":
        os._exit(0)
    if fault == "nonzero":
        os._exit(17)
    # Signature reflection loads the normal binding machinery without calling a provider.
    for value in vars(store).values():
        if inspect.isfunction(value) and value.__module__ == store.__name__:
            inspect.signature(value)
    if fault == "store-call":
        store.digest("synthetic bootstrap control")
    if fault == "extra-write":
        (owned / "data/bootstrap-unexpected.txt").write_text("synthetic unexpected write", encoding="utf-8")
    if fault == "stdout-canary":
        fixture = json.loads(Path(os.environ["CAREERBREW_DATA_FIXTURE"]).read_bytes())
        print(fixture["canary"])
    report = {
        "schema": "careerbrew-data-runtime-bootstrap-v1",
        "pid": os.getpid(),
        "driver_provenance": {"schema": "careerbrew-data-driver-provenance-v1", "pid": os.getpid(), "entry": entry, "api_complete": diagnostic_sample("api-complete")},
        "store_calls": _store_calls,
    }
    destination = Path(os.environ["CAREERBREW_DATA_REPORT"])
    expected = owned / "protocol/bootstrap-api.json"
    if destination != expected or destination.is_symlink():
        raise ValueError("bootstrap report target differs")
    descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as output:
        json.dump(report, output, ensure_ascii=True, sort_keys=True, allow_nan=False)
        output.write("\n")


def contract_fault(case_id, role, trial):
    control = os.environ.get("CAREERBREW_DATA_CONTRACT_CONTROL", "none")
    if control == "none":
        return None
    writers = {
        "json-one": ("3",),
        "json-bool": ("3",),
        "json-large": ("3",),
        "json-zero": ("3",),
        "yaml-date-map": ("4",),
        "yaml-map-date": ("4",),
        "same-wrong": ("3", "9"),
        "rewrite": ("8",),
        "plaintext": ("2",),
        "stdout": ("2",),
        "stderr": ("2",),
        "extra-write": ("2",),
    }
    valid = role == "writer" and case_id in writers.get(control, ())
    if control in ("empty", "nonzero", "hang"):
        valid = (case_id, role) in (("2", "writer"), ("1", "reader"))
    if not valid:
        raise ValueError("contract fault intent differs")
    driver = Path(os.environ["CAREERBREW_CAPTURE_ROOT"]) / "xtask/tests/fixtures/capture/data_driver.py"
    if os.fspath(driver) != __file__ or driver.is_symlink():
        raise ValueError("fault driver origin differs")
    return {
        "schema": "careerbrew-data-fault-v1",
        "control": control,
        "trial": trial,
        "case": case_id,
        "role": role,
        "pid": os.getpid(),
        "driver_sha256": hashlib.sha256(driver.read_bytes()).hexdigest(),
        "stage": "started",
        "direct_calls_observed": 0,
        "hook": None,
        "write_bytes": None,
    }


def publish_fault(fault, owned):
    path = owned / "protocol" / (fault["case"].zfill(2) + "-" + fault["role"] + "-fault.json")
    if path.parent.is_symlink() or path.is_symlink():
        raise ValueError("fault protocol path differs")
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as output:
        json.dump(fault, output, ensure_ascii=True, sort_keys=True, allow_nan=False)
        output.write("\n")


def controlled_invoke(spec, arguments, target, owned, case, role, fault):
    if fault is None or fault["control"] in ("empty", "nonzero", "hang", "plaintext", "stdout", "stderr", "extra-write"):
        return invoke(spec, arguments, target, owned, case, role)
    control = fault["control"]
    container, attribute = (store, "read_text") if control == "rewrite" else ((store.yaml, "safe_dump") if control.startswith("yaml-") else (store, "_json_text"))
    original = getattr(container, attribute)
    name = "store.read_text" if control == "rewrite" else ("yaml.safe_dump" if control.startswith("yaml-") else "store._json_text")
    hook = {"name": name, "entered": False, "completed": False, "restored": False}
    fault["hook"] = hook

    def changed(*args, **kwargs):
        if hook["entered"]:
            raise ValueError("fault hook called twice")
        hook["entered"] = True
        if control == "rewrite":
            hook["completed"] = True
            return None
        data = copy.deepcopy(args[0])
        if control in ("json-one", "same-wrong"):
            data["one"] = 2
        elif control == "json-bool":
            data["truth"] = 1
        elif control == "json-large":
            data["large"] += 1
        elif control == "json-zero":
            data["zeros"][0] = 0.0
        elif control == "yaml-date-map":
            data["date"] = {"kind": "date", "iso": "2026-03-01"}
        elif control == "yaml-map-date":
            data["tag_map"] = datetime.date(2026, 3, 1)
        result = original(data, *args[1:], **kwargs)
        hook["completed"] = True
        return result

    # The replaced serializer belongs only to this fresh Python-child invocation.
    setattr(container, attribute, changed)
    try:
        return invoke(spec, arguments, target, owned, case, role)
    finally:
        setattr(container, attribute, original)
        hook["restored"] = getattr(container, attribute) is original


def apply_retained_fault(fault, fixture, owned, target):
    if fault is None:
        return
    control = fault["control"]
    if control in ("plaintext", "stdout", "stderr"):
        payload = fixture["canary"].encode("utf-8")
        descriptor = os.open(target, os.O_WRONLY | os.O_TRUNC | os.O_NOFOLLOW) if control == "plaintext" else (1 if control == "stdout" else 2)
        try:
            fault["write_bytes"] = os.write(descriptor, payload)
        finally:
            if control == "plaintext":
                os.close(descriptor)
    elif control == "extra-write":
        descriptor = os.open(owned / "data/contract/unexpected.txt", os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        try:
            fault["write_bytes"] = os.write(descriptor, b"synthetic-unexpected-write\n")
        finally:
            os.close(descriptor)
    fault["stage"] = "api-complete" if control == "empty" else "applied"


def main():
    entry = diagnostic_sample("main-entry")
    if _bootstrap:
        bootstrap_main(entry)
        return
    if len(sys.argv) != 4:
        raise ValueError("phase arguments missing")
    trial, case_id, role = sys.argv[1:]
    if trial not in ("first", "second"):
        raise ValueError("trial differs")
    fixture_path = Path(os.environ["CAREERBREW_DATA_FIXTURE"])
    raw = fixture_path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != FIXTURE_SHA256:
        raise ValueError("fixture changed")
    fixture = json.loads(raw)
    case = next(item for item in fixture["cases"] if item["case"] == case_id)
    phase = next(item for item in case["phases"] if item["role"] == role)
    owned = Path(os.environ["CAREERBREW_DATA_OWNED"])
    if str(owned) != str(owned.resolve()):
        raise ValueError("owned root is not canonical")
    target = owned / phase["target"]
    phase_control = os.environ.get("CAREERBREW_DATA_PHASE_CONTROL", "none")
    if phase_control != "none":
        if phase_control not in ("skip", "nonzero", "hang") or (case_id, role) not in (("2", "writer"), ("1", "reader")):
            raise ValueError("phase control differs")
        if phase_control == "skip":
            os._exit(0)
        if phase_control == "nonzero":
            os._exit(17)
        import time

        while True:
            time.sleep(1)
    fault = contract_fault(case_id, role, trial)
    if fault is not None and fault["control"] in ("nonzero", "hang"):
        publish_fault(fault, owned)
        if fault["control"] == "nonzero":
            os._exit(17)
        import time

        while True:
            time.sleep(1)
    arguments = {name: decode(value) for name, value in phase["arguments"].items()}
    report = {"schema": "careerbrew-data-phase-v1", "trial": trial, "case": case_id, "role": role, "pid": os.getpid(), "fixture_sha256": FIXTURE_SHA256, "api_calls": []}
    for spec in [] if fault is not None and fault["control"] == "empty" else phase["ledger"]:
        call_arguments = arguments if spec["purpose"] == "primary" else {}
        report["api_calls"].append(controlled_invoke(spec, call_arguments, target, owned, case_id, role, fault))
    if fault is not None:
        fault["direct_calls_observed"] = len(report["api_calls"])
        apply_retained_fault(fault, fixture, owned, target)
    report["driver_provenance"] = {"schema": "careerbrew-data-driver-provenance-v1", "pid": os.getpid(), "entry": entry, "api_complete": diagnostic_sample("api-complete")}
    control = os.environ.get("CAREERBREW_DATA_DRIVER_CONTROL", "none")
    if control not in ("none", "missing-provenance") or (control != "none" and sys.argv[1:] != ["first", "1", "reader"]):
        raise ValueError("driver control differs")
    if control == "missing-provenance":
        del report["driver_provenance"]
    destination = Path(os.environ["CAREERBREW_DATA_REPORT"])
    expected = owned / "protocol" / (case_id.zfill(2) + "-" + role + "-api.json")
    if destination != expected or destination.is_symlink():
        raise ValueError("report target differs")
    descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as output:
        json.dump(report, output, ensure_ascii=True, sort_keys=True, allow_nan=False)
        output.write("\n")
    if fault is not None:
        publish_fault(fault, owned)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print("data oracle failed: " + type(error).__name__, file=sys.stderr)
        raise SystemExit(1) from None
    finally:
        if _bootstrap:
            sys.setprofile(_previous_profile)
