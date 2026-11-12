#!/usr/bin/env python3
"""Gate step: every code example in the public study deck parses.

python -> compile(); js/javascript/jsx/ts/tsx -> esbuild (from web/node_modules);
json -> json.loads; yaml -> yaml.safe_load. Other languages are not checked.
A deliberate fragment is fenced as `text`; a block holding several files marks each with a `// name.js` line. Prints card ids and languages only.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
FENCE = re.compile(r"^```(\w+)\n(.*?)^```", re.S | re.M)
FILE_MARK = re.compile(r"^// [\w./-]+\.(?:m?[jt]sx?|c[jt]s)\b.*$", re.M)  # "// main.js" starts a new file in one block
JS = {"js": "js", "javascript": "js", "jsx": "jsx", "ts": "ts", "typescript": "ts", "tsx": "tsx"}
ESBUILD = """
const esbuild = require(process.argv[1]);
const items = JSON.parse(require("fs").readFileSync(0, "utf8"));
const bad = [];
for (const it of items) {
  try { esbuild.transformSync(it.code, { loader: it.loader, format: "esm", logLevel: "silent" }); }
  catch (e) { bad.push({ id: it.id, lang: it.loader, error: String(e.errors?.[0]?.text ?? e).slice(0, 120) }); }
}
process.stdout.write(JSON.stringify(bad));
"""


def blocks() -> list[tuple[str, str, str]]:
    out = []
    for f in sorted((ROOT / "knowledge" / "cards").glob("*.yaml")):
        for c in (yaml.safe_load(f.read_text(encoding="utf-8")) or {}).get("cards", []):
            out += [(c["id"], m.group(1).lower(), m.group(2)) for m in FENCE.finditer(c["explanation"])]
    return out


def main() -> int:
    problems: list[str] = []
    js = []
    checked = 0
    for cid, lang, code in blocks():
        try:
            if lang == "python":
                compile(code, cid, "exec")
            elif lang == "json":
                json.loads(code)
            elif lang in ("yaml", "yml"):
                yaml.safe_load(code)
            elif lang in JS:
                parts = [p for p in FILE_MARK.split(code) if p.strip()] if FILE_MARK.search(code) else [code]
                js += [{"id": cid, "loader": JS[lang], "code": part} for part in parts]
                continue
            else:
                continue
            checked += 1
        except Exception as e:  # noqa: BLE001 (report any parse failure the same way)
            problems.append(f"{cid}: {lang} does not parse ({type(e).__name__}: {str(e).splitlines()[0][:100]})")
            checked += 1
    esbuild = ROOT / "web" / "node_modules" / "esbuild"
    if js and esbuild.exists():
        r = subprocess.run(["node", "-e", ESBUILD, str(esbuild)], input=json.dumps(js), capture_output=True, text=True, check=True)  # noqa: S603, S607
        problems += [f"{b['id']}: {b['lang']} does not parse ({b['error']})" for b in json.loads(r.stdout)]
        checked += len(js)
    elif js:
        print(f"skipped {len(js)} JS/TS example(s): run `cd web && npm ci` to check them")
    for p in problems:
        print(p)
    print(f"card code: {checked} example(s) checked, {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
