# Install careerbrew with your AI agent

This playbook works in Claude Code, ChatGPT.app Work/Codex, or the Codex CLI.
A person starts it with the prompt at the top of
[README.md](README.md), or with one sentence:

> Install or update careerbrew by following https://github.com/fyodoriv/careerbrew/blob/HEAD/INSTALL.md

It works for a new machine and for an update. It sets up careerbrew for a
user, and, when the person asks, for a contributor too.

Agent: follow the steps below in order. Do not skip the plan or the
questions. Use your structured question tool (`AskUserQuestion` in Claude
Code) when you have one. Give every question its full context: what the
choice is, what each option does, and what changes for the person.

## 1. Choose where the code goes

Ask: "Where should I put the careerbrew code? It is a git clone; your own
data stays in its `private/` folder, which git never tracks." Options:

- `~/apps/careerbrew` (recommended; matches the agentbrew and dotfiles defaults)
- `~/code/careerbrew`
- Another folder (let the person type a path)

Then clone it, or update it if the folder already holds a clone. An update
keeps `private/` as it is: git never tracks it, and the install below only
adds what is missing. If git asks
for a password, the person is not signed in to GitHub: run `gh auth login`
first.

```bash
git clone https://github.com/fyodoriv/careerbrew.git <DIR>   # or: git -C <DIR> pull --ff-only
```

On a corporate machine that must never push, follow
[docs/corporate-machine.md](docs/corporate-machine.md) from its step 1
instead: it gets the code and blocks pushes first.

## 2. Explain the plan

Tell the person, in your own short words:

- what they need: git, [uv](https://docs.astral.sh/uv/), and Node.js for the
  web app (optional), a few minutes, and network for the first install,
- what will change: a `.venv` in the clone, the built web app in `web/dist`,
  and their data root (`<DIR>/private`, or `$CAREERBREW_HOME`). Data records
  are encrypted at rest (`docs/privacy.md` lists the plaintext exceptions). Outside the clone, the install writes the data key
  `~/.config/careerbrew/backup.key` and fills the uv and npm caches. The
  contributor setup (`scripts/setup.sh`) also writes the `careerbrew`
  launcher in `~/.local/bin` and fills the Cargo and Playwright caches. The agent step below
  (`agentbrew sync`) also writes commands and skills into each agent's own
  config folder,
- that rerunning the install is safe: it is idempotent,
- how to undo: delete the clone (and `$CAREERBREW_HOME` if they set one). Then
  remove `~/.local/bin/careerbrew` if the setup wrote it, and take careerbrew out of your agentbrew
  setup if you added it. The data key stays in `~/.config/careerbrew/`: keep
  it while any data or backup it sealed remains, and delete it last.

If `git` or `uv` is missing, help install it first (`scripts/install.sh`
names the command), then show the plan again.

## 3. Ask for preferences

Ask these together, in one round:

1. "Which machine is this? **personal**: job hunt, interview prep, and your
   own work record. **corporate**: a work laptop; job-hunt commands are
   blocked by design and only work features run." Personal (recommended on
   your own computer) / Corporate.
2. "Do you have careerbrew plugins to install, such as your company's plugin
   and a team plugin that extends it? Each is a git URL or a folder; the
   company plugin goes first." None (recommended for a first install) / Add
   them (let the person type each one).
3. "Do you only use careerbrew, or will you also change its code? A
   **contributor** setup adds the browser for the web app's tests and runs
   the fast gate once, so you know your machine can test a change." Use it
   (recommended) / Use and contribute.
4. Corporate only: "What short label should this machine have, so a paired
   personal machine can tell it apart? For example `work-laptop`."

Map the answers to flags: the role → `--role personal|corporate`, each plugin →
`--plugin <url-or-folder>` in the order given, the label → `--label <name>`. Contribute adds step 4b.

## 4. Install

Get a clear yes to the plan, then run from the clone:

```bash
cd <DIR> && scripts/install.sh --role <role> [--plugin <url-or-folder>]... [--label <name>]
```

Always pass `--role` from an agent. Without it, on a terminal, the script asks
for the role itself (Enter keeps the current one); with no terminal it never
waits. Exit 0 means installed, 1 a missing tool or a failed step (the line
before it names the fix), 2 a bad option. On a failure, tell the person what
failed and the fix line, and offer the report in step 8.

Then check it: `scripts/doctor.sh` prints one line per check and ends with
the failed count.

### 4b. Contributor setup (only when the person contributes)

Run from the clone, one at a time:

Setup and doctor require the installed release in `rust-toolchain.toml`, with
Cargo, rustfmt and Clippy in the compiler's installation. They check locally
before setup fetches crates. They never install Rust or its components. Install
missing tools through your approved package manager, then rerun setup. Rustup
discovery disables automatic installation. A direct packaged installation also
works when its compiler is on PATH. Compiler and wrapper environment overrides
must be unset for this readiness check. `SETUP_SKIP_DEPS=1` still checks installed
tools and skips dependency fetching.

This portable check verifies the release and component availability. The
native identity probe separately
checks compiler/Clippy hashes and linkage on supported macOS installations.
Doctor does not build that probe.

```bash
scripts/setup.sh                                  # dev deps, local folders, git hooks, then the doctor
(cd web && npx playwright install chromium)       # the browser the web app's tests use
scripts/check.sh fast                             # the fast gate: lint, types, unit tests
```

Setup retains an existing Git hook path and names `hooks/pre-commit` as this
repository's entry point. It does not verify that the retained hook calls it.
The repository hook runs Lefthook's configured checks without automatic hook
installation. With no configured path, setup installs the normal local hooks.
If Git cannot read its configuration, setup stops and asks you to repair it;
it does not assume the hook path is absent.

Then point the person at [AGENTS.md](AGENTS.md) (the rules every change
follows) and `TASKS.md` (open work). Never push from a corporate machine.

## 5. Check and fix

Run `scripts/doctor.sh`. Fix every failing line with the person: each line
names its fix. Run the doctor again until it ends with 0 failed, or until
the only failures need the person (a token, a password); say which.

For configured agent clients, the doctor checks CLI availability and local
credentials with `claude auth status --json` or `codex login status`. Each
probe has a limit of two seconds and prints only a status, never account details.
An `unknown` result needs a local retry or CLI repair; the doctor never starts
login or a model call. Optional and opted-out clients do not need authentication
for shell-only use. Credential presence does not prove service access, GUI
account settings, or effective permissions.

Read [the host guide](docs/agent-hosts.md) for the configured shared memory
requirement. A failed configured recall blocks context-dependent readiness
until repair and a successful retry; deterministic CLI-only setup can continue.
Use `/careerbrew` in Claude Code. In ChatGPT.app Work/Codex, find the deployed
native entry through `agentbrew commands list --json`; use the careerbrew CLI
if the host does not expose that skill. Both follow this same playbook.

For agent workflows, install agentbrew through its setup guide, then run
`agentbrew sync` from this checkout. The Agentfile declares commands and all current
and future published `careerbrew-*` skill folders for both primary hosts. The
doctor requires actual commands, native wrappers and skill resources for configured
clients; a successful sync alone does not prove readiness. Preserve owner edits and
resolve reported source collisions before continuing. Careerbrew owns `/work`;
its plugin steps remain canonical private resources. Shell-only use can omit agent
deployment when neither primary client is configured.

## 6. Audit what the machine already has

List the career skills and commands already on this machine (for Claude
Code: `~/.claude/skills/` and `~/.claude/commands/`; `agentbrew status
--verbose` shows where each comes from). Compare each one with careerbrew's
`skills/` and `commands/`. For each, propose one action and let the person
choose:

- **adopt**: careerbrew already does it; remove the local copy,
- **merge**: the local copy does something careerbrew does not; offer to
  add it to careerbrew (a skill, a command, or a CLI action),
- **keep**: it is not career work.

From now on, extend careerbrew instead of building local copies.

## 7. Start careerbrew for the first time

Run `/careerbrew` (or `uv run careerbrew` in the clone). It starts the
watcher and the web app, refreshes, and prints what needs the person:
questions, proposals, and the next action. Walk the person through that
first screen, one item at a time: answer the questions with them, show the
web app link it printed, and do the next action it names (often: connect a
work tool, or `careerbrew init` and `careerbrew data edit profile.yaml`).

## 8. Finish

Tell the person:

- the next step the install printed (connect a work tool, then open the web
  app); `uv run careerbrew` starts everything and prints what needs them; on a
  corporate machine, follow
  [docs/corporate-machine.md](docs/corporate-machine.md) until
  `uv run careerbrew machine ready` says `corporate readiness: N/N`,
- `uv run careerbrew actions` lists every action, and the web app runs each
  one too,
- list anything in the install or the doctor that could work better, and
  offer an issue or a PR for each one,
- if anything in the install was wrong or confusing, the author is glad to
  hear it ([CONTRIBUTING.md](CONTRIBUTING.md#report-problems-and-ideas)).
  Offer to report it: `agentbrew report --repo fyodoriv/careerbrew` scrubs
  private details and shows the exact text before anything is posted; post
  only after the person says yes. Never include anything from `private/`. If
  they want to fix it themselves, offer to do it with them: a branch, the
  fix with a test and the docs, `scripts/check.sh`, then
  `agentbrew contribute` (it forks the repo and opens the PR).

## Fixed Linux observer preparation

The native developer observer uses the existing locked Serde 1.0.229 packages
with derive enabled. Doctor checks its direct dependency declaration using
`cargo metadata --locked --offline --no-deps --format-version 1`; it does not
build the observer or start Docker. Fetch the lockfile dependencies with
`cargo fetch --locked` if the offline cache is missing. The fixed Linux/arm64
observer protocol remains
a separate capability check. It does not qualify CPU/RSS measurements.
