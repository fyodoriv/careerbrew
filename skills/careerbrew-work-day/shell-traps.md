# Agent shell traps

Read this before a `/work` step runs any shell command. `careerbrew day start` prints which traps are live on this machine (`careerbrew day run --json`, `start_of_day.preflight`).

| Trap | What breaks | Safe form |
|---|---|---|
| zsh does not split words | `for f in $F` loops once, over the whole string | loop over an array (`for f in "${files[@]}"`), use `${=F}` in zsh, or run the line with `bash -c` |
| zsh `=word` expansion | `echo =====` looks up a command named `====` and fails | quote the separator: `echo '====='` |
| macOS bash 3 | `declare -A` and `mapfile` fail | a plain list, or a short Python script |
| GNU sed first on PATH | `sed -i ''` fails ("can't read ''") | edit files with the Edit tool or Python, or `perl -pi -e 's/a/b/' file` (works with both seds) |
| BSD sed | `sed -i 's/a/b/' file` fails (needs a suffix) | the same safe forms as above |
| the push guard reads the whole line | even `git fetch` can be blocked when another part of the same compound line has `$VAR` or `$(...)` (`host-undetermined`) | run each `git` or `gh` network command alone, with literal paths |
| bare `python3` | it lacks the project's packages, so tests fail on import | `uv run python -m unittest` (or `uv run pytest`), or the project's `.venv/bin/python` |

A step that runs tests names its interpreter. A step's `verify:` that starts with bare `python` or `python3` is refused (docs/plugins.md, "Work steps").
