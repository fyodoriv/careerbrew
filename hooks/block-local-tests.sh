#!/bin/bash
# careerbrew/hooks/block-local-tests.sh
#
# Hook: block-local-tests. Event: PreToolUse on Bash. Verdict: block (exit 2).
# Source rule: AGENTS.md "Rules" 1: hosted checks are the gate, so tests do
# not run on this machine.
#
# Blocks a local test run inside a careerbrew checkout: pytest (also through
# python -m, uv run, uvx), cargo test, cargo nextest, scripts/check.sh,
# scripts/e2e.sh, scripts/batch-gate.sh, scripts/tests_affected.py --run,
# tests/e2e/*.sh, evals/run.sh, npm test, npm run test* and visual*, vitest,
# playwright test, node --test and bun test. Also when started by heavy-run,
# xargs, find -exec, npx, npm exec, pnpm dlx, yarn, bunx, bash -c or eval.
# Allowed: lint, format, typecheck, build, one hook's own .test.sh, and any
# command outside a careerbrew checkout. A checkout is a folder (or a parent
# of it) whose pyproject.toml says name = "careerbrew" and that holds
# scripts/check.sh.
# Opt-out: CAREERBREW_ALLOW_LOCAL_TESTS=1, exported before the agent starts.
# Setting it inside an agent command has no effect: the hook reads its own
# environment, not the command text.
# Limits: static matching cannot catch indirection, such as `x=pytest; $x`,
# base64 text piped into sh, a shell alias or function, or a script file
# that runs tests. It is a guard rail against habit, not a sandbox.
# Adapted from agentbrew hooks/checks/block-local-tests.sh.
# Test: bash hooks/block-local-tests.test.sh

set -euo pipefail

[ "${CAREERBREW_ALLOW_LOCAL_TESTS:-}" = 1 ] && exit 0

input=$(cat)

# Fast exit: most commands hold no word that can start a test run.
case "$input" in
  *test*|*cargo*|*check.sh*|*e2e*|*batch-gate*|*run.sh*|*playwright*|*visual*|*npm*|*yarn*|*bun*|*jest*|*deno*|*xtask*|*make*) ;;
  *) exit 0 ;;
esac

_field() {
  python3 -c 'import json,sys
d=json.loads(sys.stdin.read() or "{}")
for k in sys.argv[1].split("."):
    d=d.get(k) if isinstance(d,dict) else None
print(d if isinstance(d,str) else "")' "$1" <<< "$input" 2>/dev/null || true
}
command="$(_field tool_input.command)"
[ -n "$command" ] || exit 0
cwd="$(_field cwd)"
[ -n "$cwd" ] || cwd="$PWD"

# Quoted text is data, not a command: drop heredoc bodies and replace each
# quoted string with Q. A quoted plain word (`"pytest"`, `py"test"`) is
# unquoted, since the shell runs it the same. Text that a shell runs moves to
# its own line, and is scanned again until nothing new comes out: the string
# after `bash -c`, `sh -c`, `env -S`, `eval` or `npx -c`/`npm exec -c`, a
# heredoc or here-string fed to a shell, `echo X | bash`, and `$(...)`.
# `python -c` code that names pytest counts as `pytest`. Quoted words that
# look like a path stay, so `cd "~/apps/careerbrew"` still names its folder.
_strip_text() {
  perl -0777 -ne '
    my $QS = qr/"(?:[^"\\]|\\.)*"|\x27[^\x27]*\x27/;
    my $SH = qr/(?:^|[\s|;&(\x2f])(?:ba|z|da)?sh\b/;
    sub unq { my $q = shift; $q =~ s/^(["\x27])(.*)\1$/$2/s; return $q; }
    my @queue = ($_); my @out; my $n = 0;
    while (@queue && $n++ < 64) {
      my $t = shift @queue; my @extra;

      $t =~ s/<<-?\s*(["\x27]?)(\w+)\1([^\n]*\n)((?:.*?\n)??)[ \t]*\2[ \t]*$/
        my ($rest, $body) = ($3, $4); my ($pre) = ($` =~ m{([^\n]*)\z});
        push @extra, $body if ("$pre $rest" =~ $SH); " $rest"/gmse;
      $t =~ s/\b((?:ba|z|da)?sh(?:[ \t]+-\S+)*[ \t]*)<<<[ \t]*($QS|[^\s;&|]+)/
        push @extra, unq($2); "$1"/gse;
      $t =~ s/\b(?:echo|printf)((?:[ \t]+-\w+)*)[ \t]+($QS|[^|;&\n]+?)[ \t]*\|[ \t]*(?=\S*?(?:ba|z|da)?sh\b)/
        my $x = unq($2); $x =~ s{\\n}{\n}g; push @extra, $x; "true | "/gse;
      $t =~ s/\b(?:ba|z|da)?sh[ \t]+(?:-\w+[ \t]+)*-\w*c\w*[ \t]+($QS)/
        push @extra, unq($1); "sh -c Q"/gse;
      $t =~ s/\benv((?:[ \t]+-[^S\s]\S*)*)[ \t]+-S[ \t]*($QS)/
        push @extra, unq($2); "env$1 -S Q"/gse;
      $t =~ s/\b(python[\d.]*|coverage)\b([^|;&\n]*?)[ \t]-c[ \t]*($QS)/
        my ($p, $f, $c) = ($1, $2, unq($3)); push @extra, "pytest" if $c =~ m{pytest|unittest}; "$p$f -c Q"/gse;
      $t =~ s/\b((?:npx|bunx|(?:npm|pnpm)(?:[ \t]+(?:--prefix|-w|--workspace|-C|--dir|--filter|-F|--loglevel)[ \t]+[^\s;&|]+|[ \t]+-[^\s;&|]+)*[ \t]+(?:exec|x|dlx))(?:[ \t]+-[^\s;&|]+)*?[ \t])(?:-c|--call)[ \t=]+($QS)/
        push @extra, unq($2); "${1}-c Q"/gse;
      $t =~ s/\beval[ \t]+($QS)/push @extra, unq($1); "eval Q"/gse;
      $t =~ s/($QS)/
        my $in = unq($1);
        $in =~ m{^[A-Za-z0-9_.:+-]+$} ? $in
          : $in =~ m{^[\w.~\x2f\$-]*\x2f[\w.~\x2f\$-]*$} ? $in : length($in) ? "Q" : ""/gse;
      $t =~ s/\$\(([^()]*)\)/push @extra, $1; "S"/ge;
      $t =~ s/`([^`]*)`/push @extra, $1; "S"/ge;
      push @out, $t; push @queue, @extra;
    }
    print join("\n", @out), "\n";
  '
}

# True when dir (or a parent of it) is a careerbrew checkout.
_is_careerbrew_checkout() {
  local d="$1"
  # shellcheck disable=SC2016,SC2088  # the patterns match literal text
  case "$d" in
    "~") d="$HOME" ;;
    "~/"*) d="$HOME/${d#\~/}" ;;
    '$HOME'|'${HOME}') d="$HOME" ;;
    '$HOME/'*) d="$HOME/${d#\$HOME/}" ;;
    '${HOME}/'*) d="$HOME/${d#\$\{HOME\}/}" ;;
  esac
  case "$d" in /*) ;; *) d="$cwd/$d" ;; esac
  while [ -n "$d" ] && [ "$d" != / ]; do
    if [ -f "$d/scripts/check.sh" ] && [ -f "$d/pyproject.toml" ] \
      && grep -qE '^name[[:space:]]*=[[:space:]]*"careerbrew"' "$d/pyproject.toml"; then
      return 0
    fi
    d="${d%/*}"
  done
  return 1
}

stripped="$(printf '%s\n' "$command" | _strip_text)"

# Folders the command may run in: the session folder plus every `cd X`,
# `-C X`, `--prefix X`, `--dir X`, `--directory X` and `--manifest-path X`.
dirs=("$cwd")
while IFS= read -r d; do
  [ -n "$d" ] && dirs+=("$d")
done < <(printf '%s\n' "$stripped" \
  | perl -ne 'while (/(?:^|[;&|(]\s*|\s)(?:cd|-C|--prefix|--dir|--directory|--manifest-path|--project)[ \t=]+([^\s;&|()]+)/g) { print "$1\n" }
      while (/(?:^|[\s=;&|(])((?:\x2f|~\x2f|\$\{?HOME\}?\x2f)[^\s;&|()]+)/g) { print "$1\n" }')
# Folders named by path arguments (any `/x` or `~/x` word), so
# `pytest ~/apps/careerbrew/tests` is judged by that folder.

# True when an npm script name runs tests.
_is_test_script() {
  case "$1" in
    test|test:*|t|visual|visual:*) return 0 ;;
  esac
  return 1
}

# True when one simple command runs a test suite.
_is_test_run() {
  local -a w
  read -r -a w <<< "$1" || true
  while [ "${#w[@]}" -gt 0 ]; do
    if [[ "${w[0]}" == *=* ]]; then w=("${w[@]:1}"); continue; fi
    case "${w[0]##*/}" in
      if|while|until|then|do|else|elif|'!'|time|nohup|exec|command|builtin|caffeinate|eval|heavy-run|--exclusive|setsid|stdbuf|arch)
        w=("${w[@]:1}") ;;
      env|sudo|watch|script) local wrap="${w[0]##*/}"; w=("${w[@]:1}")
        while [ "${#w[@]}" -gt 0 ] && [[ "${w[0]}" == -* || "${w[0]}" == *=* ]]; do
          case "$wrap:${w[0]}" in
            env:-u|env:-C|env:-S|env:--unset|env:--chdir|sudo:-u|sudo:-g|sudo:-C|watch:-n|watch:--interval) w=("${w[@]:2}") ;;
            *) w=("${w[@]:1}") ;;
          esac
        done
        # `script -q FILE CMD` writes to FILE and runs CMD.
        [ "$wrap" = script ] && [ "${#w[@]}" -gt 0 ] && w=("${w[@]:1}") ;;
      nice|timeout) w=("${w[@]:1}")
        while [ "${#w[@]}" -gt 0 ] && [[ "${w[0]}" == -* || "${w[0]}" =~ ^[0-9.]+[smhd]?$ ]]; do w=("${w[@]:1}"); done ;;
      -*) w=("${w[@]:1}") ;;
      *) break ;;
    esac
  done
  [ "${#w[@]}" -gt 0 ] || return 1
  local word="${w[0]}" tool="${w[0]##*/}" a
  # Gate and story scripts, by their path in the repo.
  case "$word" in
    *scripts/check.sh)
      # `scripts/check.sh inputs <step>` only prints a step's inputs.
      [ "${w[1]:-}" = inputs ] && return 1
      return 0 ;;
    *scripts/e2e.sh|*scripts/batch-gate.sh|*evals/run.sh|*tests/e2e/*.sh) return 0 ;;
    *scripts/tests_affected.py)
      for a in "${w[@]:1}"; do [ "$a" = --run ] && return 0; done
      return 1 ;;
  esac
  case "$tool" in
    pytest|py.test) return 0 ;;
    python|python3|python3.*)
      local i
      for i in "${!w[@]}"; do
        [ "${w[$i]}" = -m ] && [ "${w[$((i + 1))]:-}" = pytest ] && return 0
        [ "${w[$i]}" = -mpytest ] && return 0
      done
      # `python scripts/tests_affected.py --run` runs pytest.
      _is_test_run "${w[*]:1}"
      return ;;
    coverage)
      # `coverage run -m pytest` runs pytest.
      [ "${w[1]:-}" = run ] || return 1
      _is_test_run "python ${w[*]:2}"; return ;;
    rustup)
      # `rustup run <toolchain> X` starts X.
      [ "${w[1]:-}" = run ] || return 1
      _is_test_run "${w[*]:3}"; return ;;
    pipx)
      [ "${w[1]:-}" = run ] || return 1
      _is_test_run "uvx ${w[*]:2}"; return ;;
    make|gmake)
      for a in "${w[@]:1}"; do case "$a" in -n|--dry-run|--just-print|--recon) return 1 ;; esac; done
      for a in "${w[@]:1}"; do case "$a" in check|test|test-*|test_*|e2e|e2e-*) return 0 ;; esac; done
      return 1 ;;
    jest) return 0 ;;
    deno) [ "${w[1]:-}" = test ] && return 0; return 1 ;;
    uv)
      # `uv run [flags] X` and `uv tool run X` start X. Common flags with a
      # value come first.
      w=("${w[@]:1}")
      while [ "${#w[@]}" -gt 0 ] && [[ "${w[0]}" == -* ]]; do
        case "${w[0]}" in
          --project|--directory|--python|-p|--config-file|--cache-dir) w=("${w[@]:2}") ;;
          *) w=("${w[@]:1}") ;;
        esac
      done
      [ "${w[0]:-}" = tool ] && [ "${w[1]:-}" = run ] && { _is_test_run "uvx ${w[*]:2}"; return; }
      [ "${w[0]:-}" = run ] || return 1
      w=("${w[@]:1}")
      while [ "${#w[@]}" -gt 0 ] && [[ "${w[0]}" == -* ]]; do
        case "${w[0]}" in
          --with|--with-requirements|--with-editable|--python|-p|--project|--directory|--package|--extra|--group|--env-file|--index|--index-url) w=("${w[@]:2}") ;;
          *) w=("${w[@]:1}") ;;
        esac
      done
      [ "${#w[@]}" -gt 0 ] || return 1
      _is_test_run "${w[*]}"
      return ;;
    cargo)
      w=("${w[@]:1}")
      while [ "${#w[@]}" -gt 0 ] && [[ "${w[0]}" == -* || "${w[0]}" == +* ]]; do
        case "${w[0]}" in
          --manifest-path|--config|-Z|--color) w=("${w[@]:2}") ;;
          *) w=("${w[@]:1}") ;;
        esac
      done
      case "${w[0]:-}" in test|t|nextest) return 0 ;; esac
      # `cargo xtask test` runs the native test tasks.
      [ "${w[0]:-}" = xtask ] && [ "${w[1]:-}" = test ] && return 0
      return 1 ;;
    npm|pnpm|yarn)
      # Global flags (and the values of the common ones) may come first.
      w=("${w[@]:1}")
      while [ "${#w[@]}" -gt 0 ] && [[ "${w[0]}" == -* ]]; do
        case "${w[0]}" in
          --prefix|-w|--workspace|-C|--dir|--filter|-F|--loglevel|--cwd) w=("${w[@]:2}") ;;
          *) w=("${w[@]:1}") ;;
        esac
      done
      [ "${#w[@]}" -gt 0 ] || return 1
      # `yarn workspace web test` runs the web workspace's script.
      if [ "${w[0]}" = workspace ]; then w=("${w[@]:2}"); [ "${#w[@]}" -gt 0 ] || return 1; fi
      case "${w[0]}" in
        # `npm exec X`, `npm x X` and `pnpm dlx X` start X, like npx.
        exec|x|dlx) _is_test_run "npx ${w[*]:1}"; return ;;
        test|t) return 0 ;;
        run|run-script)
          w=("${w[@]:1}")
          while [ "${#w[@]}" -gt 0 ] && [[ "${w[0]}" == -* ]]; do w=("${w[@]:1}"); done
          [ "${#w[@]}" -gt 0 ] || return 1
          _is_test_script "${w[0]}"; return ;;
      esac
      # `yarn test` and `pnpm visual` run the script without `run`.
      # `yarn vitest` and `pnpm vitest` start the tool, like npx.
      if [ "$tool" != npm ]; then
        _is_test_script "${w[0]}" && return 0
        _is_test_run "npx ${w[*]}"; return
      fi
      return 1 ;;
    xargs|npx|bunx|uvx|parallel)
      # A launcher: skip its own flags (and their values), then judge the
      # command it starts.
      w=("${w[@]:1}")
      while [ "${#w[@]}" -gt 0 ] && [[ "${w[0]}" == -* ]]; do
        case "$tool:${w[0]}" in
          npx:-p|npx:--package|bunx:-p|bunx:--package|uvx:--from|uvx:--with|uvx:--python|xargs:-[adEIJLnPRSs]|xargs:--arg-file|xargs:--delimiter|xargs:--max-args|xargs:--max-chars|xargs:--max-procs) w=("${w[@]:2}") ;;
          # GNU `--replace` takes no separate value (`--replace=R` is one word).
          *) w=("${w[@]:1}") ;;
        esac
      done
      [ "${#w[@]}" -gt 0 ] || return 1
      _is_test_run "${w[*]}"
      return ;;
    find)
      # `find ... -exec CMD ...` starts CMD.
      local i
      for i in "${!w[@]}"; do
        case "${w[$i]}" in -exec|-execdir|-ok|-okdir)
          _is_test_run "${w[*]:$((i + 1))}" && return 0 ;;
        esac
      done
      return 1 ;;
    vitest)
      # A lone version or help flag, or `vitest list`, runs no tests.
      case "${w[*]:1}" in --version|-v|--help|-h|list|list\ *) return 1 ;; esac
      return 0 ;;
    playwright)
      [ "${w[1]:-}" = test ] && return 0
      return 1 ;;
    bun)
      [ "${w[1]:-}" = test ] && return 0
      [ "${w[1]:-}" = run ] && { _is_test_script "${w[2]:-}"; return; }
      return 1 ;;
    node|tsx|ts-node)
      for a in "${w[@]:1}"; do
        case "$a" in --test|--test=*) return 0 ;; esac
      done
      return 1 ;;
    bash|sh|zsh)
      # `bash scripts/check.sh fast` runs the gate; `bash -n X` only checks syntax.
      for a in "${w[@]:1}"; do
        case "$a" in -n|-*n*) [[ "$a" != --* ]] && return 1 ;; -*) ;; *) break ;; esac
      done
      _is_test_run "${w[*]:1}"
      return ;;
  esac
  return 1
}

hit=""
while IFS= read -r line; do
  while IFS= read -r seg; do
    seg="${seg#"${seg%%[![:space:]]*}"}"
    [ -n "$seg" ] || continue
    if _is_test_run "$seg"; then hit="$seg"; break 2; fi
  # An empty {} is an xargs or find placeholder, not a brace group.
  done < <(printf '%s\n' "$line" | sed -E 's/\{\}/ARG/g; s/(&&|\|\||;|\||[(){}])/\n/g')
done <<< "$stripped"
[ -n "$hit" ] || exit 0

for d in "${dirs[@]}"; do
  if _is_careerbrew_checkout "$d"; then
    cat >&2 <<'MSG'
BLOCKED: careerbrew tests run on hosted CI only, not on this machine.
Run lint, format or a type check locally (`uv run ruff check .`, `uv run pyright`,
`npm run typecheck`). Then push the branch and read the hosted checks:
  gh pr checks <pr> --watch
  gh run view <run-id> --log-failed
Opt-out: export CAREERBREW_ALLOW_LOCAL_TESTS=1 before the agent starts.
See docs/ci.md ("Tests run on hosted CI only").
MSG
    exit 2
  fi
done
exit 0
