"""Real-clock source-health browser fixture; synthetic invocation-owned data only."""

from __future__ import annotations

import contextlib
import io
import json
import os
import signal
import sys
import tempfile
import threading
from datetime import UTC, datetime, timedelta
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))


def fixture_timezone(instant: datetime) -> str:
    """Choose a fixed-offset day with noon at fixture setup; keep the real instant."""
    offset = 12 - instant.astimezone(UTC).hour
    if not offset:
        return "Etc/GMT"
    return f"Etc/GMT{'-' if offset > 0 else '+'}{abs(offset)}"


def main() -> None:
    from careerbrew import calendar, cli, config, server
    from careerbrew.work import standup

    with tempfile.TemporaryDirectory(prefix="careerbrew-health-browser-") as scratch:
        root = Path(scratch)
        os.environ.update(
            {
                "HOME": str(root / "client"),
                "CAREERBREW_HOME": str(root / "data"),
                "CAREERBREW_EVENTS": "off",
                "CAREERBREW_LLM": "off",
                "CAREERBREW_NOTIFY": "off",
                "CAREERBREW_POSTHOG_KEY": "",
                "CAREERBREW_TELEMETRY_URL": "http://127.0.0.1:9",
                "CAREERBREW_KEY_FILE": str(root / "key"),
                "CAREERBREW_PAIR_KEY_FILE": str(root / "pair"),
                "CAREERBREW_BACKUP_DIRS": str(root / "backups"),
                "CAREERBREW_CLAUDE_CONFIG": str(root / "no-claude.json"),
                "CAREERBREW_CODEX_CONFIG": str(root / "no-codex.toml"),
                "CAREERBREW_COMMANDS_DIR": str(root / "commands"),
                "XDG_STATE_HOME": str(root / "state"),
            }
        )
        (root / "client").mkdir(mode=0o700)
        with contextlib.redirect_stdout(io.StringIO()):
            cli.main(["init"])
        config.set_value("live_source_max_age_minutes", "1")
        config.set_value("feed_tool_sources", "mcp__pages__*=pagerduty")
        now = calendar.now_utc()
        config.set_value("timezone", fixture_timezone(now))
        os.environ["CAREERBREW_NOW"] = (now - timedelta(minutes=61)).isoformat()
        standup.save_rows('{"rows":[{"title":"Synthetic incident","aliases":["incident"],"owner":"me"}]}')
        os.environ.pop("CAREERBREW_NOW")
        for name, ident, title in [("initial", "health-first", "Resolved first synthetic incident"), ("update", "health-second", "Resolved second synthetic incident")]:
            item = {"id": ident, "source": "pagerduty", "kind": "pagerduty", "at": now.isoformat(), "title": title, "actor": "me", "status": "resolved"}
            (root / f"{name}.jsonl").write_text(json.dumps(item) + "\n")
        app, thread = server.serve_in_thread(0, "e2e-token-not-secret")
        stop = threading.Event()
        signal.signal(signal.SIGINT, lambda *_args: stop.set())
        signal.signal(signal.SIGTERM, lambda *_args: stop.set())
        print(f"careerbrew live health fixture: http://127.0.0.1:{app.server_address[1]}/ root={root}", flush=True)
        try:
            stop.wait()
        finally:
            app.shutdown()
            app.server_close()
            thread.join(timeout=5)


if __name__ == "__main__":
    main()
