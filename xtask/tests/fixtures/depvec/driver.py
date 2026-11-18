"""rust-01 dependency vector oracle: run one vector set with careerbrew's own code.

Usage: python -B driver.py <set> <vectors-file>. Prints one JSON line,
{"set": ..., "results": [{"id": ..., "output": {"ok": value} | {"error": true}}]},
in the typed value form `crates/depvec/src/canon.rs` describes.

To add a set: a function below and one line in SETS.
"""

import datetime
import json
import struct
import sys

import fsrs
import yaml

from careerbrew import store
from careerbrew.tutor import cards


def encode(value):
    if value is None:
        return {"t": "null"}
    if isinstance(value, bool):
        return {"t": "bool", "v": value}
    if isinstance(value, int):
        return {"t": "int", "v": str(value)}
    if isinstance(value, float):
        return {"t": "float", "v": struct.pack(">d", value).hex()}
    if isinstance(value, str):
        return {"t": "str", "v": value}
    if isinstance(value, datetime.datetime):
        return {"t": "datetime", "v": value.isoformat()}
    if isinstance(value, datetime.date):
        return {"t": "date", "v": value.isoformat()}
    if isinstance(value, list):
        return {"t": "list", "v": [encode(item) for item in value]}
    if isinstance(value, dict):
        return {"t": "map", "v": [[encode(k), encode(v)] for k, v in value.items()]}
    raise TypeError("unsupported value type")


def decode(value):
    kind, v = value["t"], value.get("v")
    if kind == "null":
        return None
    if kind in ("bool", "str"):
        return v
    if kind == "int":
        return int(v)
    if kind == "float":
        return struct.unpack(">d", bytes.fromhex(v))[0]
    if kind == "date":
        return datetime.date.fromisoformat(v)
    if kind == "datetime":
        return datetime.datetime.fromisoformat(v)
    if kind == "list":
        return [decode(item) for item in v]
    if kind == "map":
        return {decode(k): decode(item) for k, item in v}
    raise ValueError("unknown value type")


def _yaml_load(text):
    # store.read_yaml without the file and the default.
    return yaml.load(text, Loader=store._SAFE_LOADER)  # noqa: S506 (the safe loader store.py uses)


def json_yaml(op, data):
    if op == "json-read":
        return encode(json.loads(data))
    if op == "json-write":
        return store._json_text(decode(data))
    if op == "yaml-read":
        return encode(_yaml_load(data))
    if op == "yaml-roundtrip":
        # The text store.write_yaml writes.
        return encode(_yaml_load(yaml.safe_dump(decode(data), sort_keys=False, allow_unicode=True)))
    raise ValueError("unknown op")


# A fixed synthetic start; review times are minutes after it.
FSRS_START = datetime.datetime(2026, 1, 5, 9, 0, tzinfo=datetime.UTC)


def _scheduler():
    # cards.review calls fsrs.Scheduler() with its defaults; fuzzing is pinned
    # off, since py-fsrs draws it from random() and a vector must be fixed.
    return fsrs.Scheduler(enable_fuzzing=False)


def fsrs_schedule(op, data):
    if op == "parameters":
        if data is not None:
            raise ValueError("parameters takes no input")
        scheduler = _scheduler()
        return {
            "parameters": [f"{w:.4f}" for w in scheduler.parameters],
            "desired_retention": f"{scheduler.desired_retention:.2f}",
            "learning_steps_seconds": [int(s.total_seconds()) for s in scheduler.learning_steps],
            "relearning_steps_seconds": [int(s.total_seconds()) for s in scheduler.relearning_steps],
            "maximum_interval_days": scheduler.maximum_interval,
        }
    if op == "schedule":
        scheduler = _scheduler()
        card = fsrs.Card()
        out = []
        for review in data["reviews"]:
            at = FSRS_START + datetime.timedelta(minutes=review["at_minutes"])
            card, _log = scheduler.review_card(card, cards.RATINGS[review["rating"]], review_datetime=at)
            out.append(
                {
                    "state": card.state.name.lower(),
                    "step": card.step,
                    "due_seconds": int((card.due - FSRS_START).total_seconds()),
                    "stability": f"{card.stability:.2f}",
                    "difficulty": f"{card.difficulty:.2f}",
                }
            )
        return out
    raise ValueError("unknown op")


# The registry: one line per vector set.
SETS = {"json-yaml": json_yaml, "fsrs": fsrs_schedule}


def main():
    name, path = sys.argv[1:]
    run = SETS[name]
    with open(path, encoding="utf-8") as fh:
        fixture = json.load(fh)
    results = []
    for vector in fixture["vectors"]:
        try:
            output = {"ok": run(vector["op"], vector["input"])}
        except Exception:  # noqa: BLE001 (any failure is the outcome "error")
            output = {"error": True}
        results.append({"id": vector["id"], "output": output})
    print(json.dumps({"set": name, "results": results}))


if __name__ == "__main__":
    main()
