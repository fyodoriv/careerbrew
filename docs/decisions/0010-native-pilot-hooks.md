# 0010: The native pilot writes the event row and never sends telemetry

Date: 2026-11-18. Status: accepted. Step `rust-03` of [the Rust migration](../rust-migration.md).

## Context

`careerbrew actions` in Python ends with two hooks in `cli.main`:

- `events.emit("cli.run", ...)` appends one content-free row to `logs/events.jsonl` in the data folder (plaintext; it is skipped when `CAREERBREW_EVENTS=off`).
- `telemetry.maybe_send()` tries, at most once a day, to send queued rows to PostHog. It reads and writes the encrypted store.

The native pilot binary `careerbrew-native` serves `actions` and static help without Python. It must not read the vault or the encrypted settings. It still has to leave the same trace, or the product tiles stop counting `actions`.

## Decision

- After a successful `actions` (exit 0), the binary appends the row Python writes: keys `command`, `event`, `ms`, `status`, `ts`, `v`, `via`, sorted as `json.dumps(sort_keys=True)` sorts them. `command` is `actions`, `event` is `cli.run`, `status` is `ok`, `ms` is its own run time, `ts` is UTC seconds, `v` is the version in `pyproject.toml`, and `via` is `agent` when `CLAUDECODE` or `CODEX_SANDBOX` is set, else `cli`. It never writes free text.
- It writes nothing when `CAREERBREW_EVENTS=off`, on an argparse usage error, or for help, as Python does.
- It never calls the telemetry send. The next Python command sends the queued rows, because `telemetry.pending` reads the local log from its last sent offset, whoever appended the rows.
- Its capabilities come from the environment only (`events::Caps`: the data root and whether events are on), so nothing in the crate can reach the store.
- When it cannot resolve the data root the way Python does (an empty `CAREERBREW_HOME`, or one that starts with `~`) and events are on, it exits 64 and writes nothing, so the route falls back to Python.

## Consequences

- A user who only runs native commands sends no telemetry until a Python command runs. Today no route sends a command to the binary, and the launcher still starts Python, so the delay is at most until the next Python command.
- The `v` field reads `pyproject.toml` at build time. Python reads the installed package metadata, which falls back to `0.0.0` in a copy that was never installed. The two agree in a normal install.
- `ms` measures the native run, so it is smaller than Python's. That is the real cost the user saw.
- Tests: `crates/cbcli/tests/cli.rs` (`events_on_appends_one_content_free_row`). A hand comparison with events on gave the same file list and the same row keys and values apart from `ts` and `ms`.
