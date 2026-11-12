"""Fixed synthetic child for the watch-cycle resource test."""

from __future__ import annotations

import json
import os
import resource
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
MARKER = "owned watch-cycle fixture v1\n"
PRESSURE_MIB = 144


def environment(root: Path) -> dict[str, str]:
    values = {
        "HOME": "home",
        "TMPDIR": "tmp",
        "XDG_CONFIG_HOME": "config",
        "XDG_CACHE_HOME": "cache",
        "XDG_STATE_HOME": "state",
        "CAREERBREW_HOME": "data",
        "CAREERBREW_KEY_FILE": "key",
        "CAREERBREW_PAIR_KEY_FILE": "pair",
        "CAREERBREW_COMMANDS_DIR": "commands",
        "CAREERBREW_CLAUDE_CONFIG": "claude.json",
        "CAREERBREW_CODEX_CONFIG": "codex.toml",
        "CAREERBREW_BACKUP_DIRS": "backups",
        "CAREERBREW_PUBLIC_CARDS": "cards",
    }
    return {
        **{name: str(root / path) for name, path in values.items()},
        "PATH": os.defpath,
        "LC_CTYPE": "C.UTF-8",
        "CAREERBREW_NOW": "2026-11-02T09:00:00-04:00",
        "CAREERBREW_POSTHOG_KEY": "",
        "CAREERBREW_NOTIFY": "off",
        "CAREERBREW_LLM": "off",
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "on",
        "DO_NOT_TRACK": "1",
    }


def command(pressure: int) -> list[str]:
    version = f"python{sys.version_info.major}.{sys.version_info.minor}"
    site = Path(sys.executable).parent.parent / "lib" / version / "site-packages"
    return [sys.executable, "-I", "-S", "-B", __file__, str(site), str(pressure)]


def rss_mib() -> float:
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / (1024 * 1024 if sys.platform == "darwin" else 1024)


def main() -> None:
    if not (sys.flags.isolated and sys.flags.no_site and sys.dont_write_bytecode) or len(sys.argv) != 3:
        raise SystemExit("watch fixture startup refused")
    site, pressure = Path(sys.argv[1]), sys.argv[2]
    root = Path.cwd()
    if pressure not in ("0", str(PRESSURE_MIB)) or str(site) != command(0)[5] or not site.is_dir() or root.name != "watch-cycle" or (root / ".owner").read_text(encoding="utf-8") != MARKER:
        raise SystemExit("watch fixture input refused")
    if sys.platform == "darwin":
        os.environ.pop("__CF_USER_TEXT_ENCODING", None)
    if dict(os.environ) != environment(root):
        raise SystemExit("watch fixture environment refused")

    denied = []

    def audit(event, _args):
        if event.startswith("socket.") or event in {"subprocess.Popen", "os.system", "os.posix_spawn", "os.exec", "os.fork", "os.forkpty", "pty.spawn"}:
            denied.append("process-or-network")
            raise PermissionError("watch fixture blocks process or network access")

    sys.addaudithook(audit)
    startup = rss_mib()
    sys.path.extend((str(site), str(REPO / "src")))
    from careerbrew import store
    from careerbrew.day import watch
    from careerbrew.work import evidence

    allocation = None
    if pressure != "0":
        allocation = bytearray(PRESSURE_MIB * 1024 * 1024)
        for offset in range(0, len(allocation), os.sysconf("SC_PAGE_SIZE")):
            allocation[offset] = 1
    try:
        evidence.note("feat: shipped search", ["delivery"])
        before_cycle = rss_mib()
        first = watch.run_cycle()
        second = watch.run_cycle()
        cursors = store.read_json(watch.cursors_path())
        record = {
            "pid": os.getpid(),
            "ppid": os.getppid(),
            "pgid": os.getpgrp(),
            "sid": os.getsid(0),
            "startup_rss_mib": startup,
            "before_cycle_rss_mib": before_cycle,
            "pressure_mib": int(pressure),
            "cycles": [{key: cycle[key] for key in ("added", "cpu_ms", "rss_mb")} for cycle in (first, second)],
            "cursors": {name: cursors[name]["ok"] for name in ("evidence", "calendar")},
            "denied": denied,
        }
    finally:
        del allocation
    print(json.dumps(record), flush=True)


if __name__ == "__main__":
    main()
