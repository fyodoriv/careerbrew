# Corporate machine: pick up careerbrew and build the company plugin

This is the runbook for an agent, or you, on a corporate laptop.

Your company already has a plugin? Go to [A second person at the company](#a-second-person-at-the-company) to reuse it. Otherwise, follow the steps below to build the first plugin.
The `corporate-setup` skill runs these steps. The `corporate-plugin` skill runs step 3 and step 5. In Claude Code, `/careerbrew-corporate` starts both.

**Done when** `careerbrew machine ready` prints `corporate readiness: N/N`.

## Rules first

- Get your employer's approval before you install the tool or create tokens.
- Job hunting never happens on this machine. careerbrew blocks job-hunt commands there.
- Company content lives only in the plugin repo `careerbrew-<org>`. That repo is private and sits on the company's git host or in a local folder.
- Never put company content in the careerbrew repo. This machine only pulls careerbrew; it never pushes.
- Tokens go only into the hidden prompt in your own terminal. Never paste them into chat or files.
- Anything an agent reads with its own tools (work chat, tickets, mail, links, internal docs) goes to that agent's provider. Use those agent flows (the live feed, the work companion, promotion research on an internal ladder) only when your employer approved that provider for that data. careerbrew itself sends no work data to a provider. The plugin policy field `agent_may_read_business` (default `false`) limits what careerbrew itself shows or writes for an agent (command output, watcher research, project memory notes); it does not limit the agent's own tools.
- Read internal docs (ladders, wikis) only for the plugin. Keep them out of chat logs that leave the machine. Your company's AI policy sets `agent_may_read_business`.

## 1. Get the code

Pick the option your company allows.

- **A. Clone from GitHub** (needs GitHub access from the work machine):

  ```bash
  gh repo clone <owner>/careerbrew ~/apps/careerbrew   # the address is in README.md
  ```

- **B. Copy a git bundle from the personal machine** (no GitHub access needed):

  ```bash
  git -C ~/apps/careerbrew bundle create ~/careerbrew.bundle main
  ```

  Move `careerbrew.bundle` over an approved channel. Then, on the corporate machine:

  ```bash
  git clone ~/careerbrew.bundle ~/apps/careerbrew
  ```

  To update later, make a new bundle and run `git -C ~/apps/careerbrew pull ~/careerbrew.bundle main`.

Then block pushes from this machine:

```bash
git -C ~/apps/careerbrew remote set-url --push origin no-push-from-corporate
```

`careerbrew checkup` treats any push URL that is not a real remote (a URL, `user@host:path`, or an existing folder) as blocked, so this value passes its pull-only step.

## 2. Install with the corporate role

```bash
cd ~/apps/careerbrew && scripts/install.sh --role corporate --label work-laptop
```

The installer writes a work-only `profile.yaml`, with no salary or job-search fields.
Set `name` and `employer` with `uv run careerbrew data set profile.yaml --name "<your name>" --employer "<your employer>"`, or use `uv run careerbrew data edit profile.yaml` in a terminal. Replace the placeholders with your own values. Both become leak terms. Command arguments can appear in local process listings and shell history; do not use this action for secrets.

## 3. Build the company plugin

```bash
uv run careerbrew plugin new <org> --connect jira,github,slack,gdocs,zoom --jira-url https://jira.<org>.example.com --github-user <login>
```

This creates `~/careerbrew-plugins/careerbrew-<org>/` with git, safe policy defaults, and Keychain token references.
It also sets `ladder: <org>-eng` in `settings.yaml`.
On the corporate role, `policy.allowed_hosts` holds only the hosts from `--jira-url` and `--github-api` plus the hosts of the tools you connect (Slack, Google Docs, Zoom); no personal mail or public GitHub host.
On a real terminal it asks for each tool's token with a hidden prompt. When an agent runs it (no terminal), it prints one `careerbrew secret set careerbrew.p-<id>.<tool>` command per tool instead; you run those in step 4.
Then fill it in. These files are the real work:

| File | What goes in | How |
|---|---|---|
| `ladders/<org>-eng.yaml` | the company's real levels, competencies, and expectations | `promo-research` skill on the internal ladder doc; keep the `generic-swe` shape and keep `id: <org>-eng` (settings point to it) |
| `sensitive-terms.txt` | the company name, codenames, customers, internal hosts, people | one per line; at least two |
| `careerbrew-plugin.yaml` `policy` | allowed model routes, backup targets, `agent_may_read_business`, `public_metrics` | match the company's AI and data policy; stricter is safer |
| `evidence.yaml` | git folders and connectors to collect from | add internal git hosts (`--api https://<ghe>/api/v3` for GitHub Enterprise) |
| `rules.md` | house rules the agent must follow | short bullets |

Set `target_level` with `uv run careerbrew data edit settings.yaml` to one of the company ladder's level names, for example `L5`.

Commit the plugin, then reinstall it. `plugin refresh` reads the folder from the private lock, so you never type the company name (careerbrew redacts it from output). Run it after every plugin edit: readiness reads the installed copy, not your folder. If you move the folder, run `plugin add <new folder>` once.

```bash
git -C ~/careerbrew-plugins/careerbrew-<org> add -A
git -C ~/careerbrew-plugins/careerbrew-<org> commit -m "feat: company plugin"
uv run careerbrew plugin refresh
```

If you want a backup, push the plugin repo to the company git host as a private repo. Never push it to a personal host.
A team-specific plugin can `extends: <org>`. See [plugins.md](plugins.md).

## 4. Connect the tools

You type each token yourself, in your own terminal. Pick the line that fits:

- `plugin new` already asked for the tokens: nothing to do.
- `plugin new` lists `careerbrew secret set ...` commands on its run page (`uv run careerbrew report latest`): run each one and paste the token into the hidden prompt.
- A tool that is not in the plugin: `uv run careerbrew connect <tool>`, for example `connect jira --base-url https://jira.<org>.example.com --email <you>`. If 1Password is mandated, add `--ref op://<vault>/<item>/<field>`.

Then run `uv run careerbrew work collect`. Its summary line should say `0 source error(s)`; each error is a blocker with its fix in `uv run careerbrew report latest`.

## 5. Prove it

```bash
uv run careerbrew machine ready
```

Its summary line says `corporate readiness: N/N`. Each failed check is a blocker on its run page (`uv run careerbrew report latest`) and names its fix. Then run `uv run careerbrew serve` and open the Today and Work views.

## 6. Pair with the personal machine (optional)

Run `careerbrew pair init` on one machine and `careerbrew pair join` on the other.
Store the key in your password manager. Pairing only moves settings from the personal machine to this one; nothing moves out (see below and [integrations.md](integrations.md)).

## What `machine ready` checks

| Check | Why |
|---|---|
| role is corporate | job-hunt commands are blocked |
| `profile.yaml` has no job-hunt fields | data minimization on employer hardware |
| your name and employer are set | they become leak terms |
| no job-hunt folders | job-hunt data never sits on this machine (an empty `jobs/`, `tutor/`, `prep/`, or `linkedin/` folder holds none, and nor do Finder's `.DS_Store` and lock files, so it passes; a temp file left by a crashed write still fails it) |
| a plugin is installed | company content has a home |
| installed plugins match their lock hashes | no tampering |
| a plugin ladder differs from the generic copy | the promo doc uses real criteria |
| the active ladder comes from a plugin | the promo doc uses the company ladder |
| `target_level` is one of that ladder's levels | the promo doc measures the right bar |
| the plugin evidence file holds no per-person values (fix: `machine split-plugin-evidence`) | a coworker never inherits your email, login, or repos. The check reads the evidence file only: a team roster (`live/team.yaml`) holds coworkers' names by design, see [plugins](plugins.md) |
| at least two sensitive terms (comments do not count) | the leak gate knows what to block |
| the careerbrew repo has none of the plugin's terms | company content stays out of the base repo |

## careerbrew keeps corporate data on this machine

careerbrew never copies corporate data to your personal machine or to a hosted model. It refuses a backup into, and a restore to, a cloud-synced folder it knows, and `machine ready` reports a data folder in one; it does not move the folder for you. Two things do leave, both by your choice: a message you send yourself (a PM update to your company Slack or a mail draft, on your click), and anything an agent's own tools read (the agent rule in [Rules first](#rules-first)). The guards:

- `sync export` refuses here. Only settings (`ladder`, `target_level`) move from your personal machine to this one.
- Keep the data folder out of iCloud Drive, iCloud Desktop & Documents, OneDrive, Dropbox, and Google Drive. `machine ready` fails when it is in one. Fix: `export CAREERBREW_HOME=~/careerbrew-data` and move the folder.
- Backups go only to local folders. A cloud-synced target is refused.
- Models: careerbrew itself sends business data only to a local route (Ollama on this machine).
- Telemetry: anonymous usage counts go to PostHog EU only when you set a key. The send obeys the plugin host policy: with `allowed_hosts` set and the telemetry host not on it, nothing is sent. A plugin policy `telemetry: false` also turns it off ([privacy.md](privacy.md#data-exits-and-their-guards)).
- `careerbrew checkup` checks all of this in its "no cloud-synced data or backups" step.

## A second person at the company

A coworker sets up their own corporate machine with the same company plugin, in one command, and nothing of yours reaches them:

```bash
git clone <careerbrew repo> && cd careerbrew && git remote set-url --push origin no-push-from-corporate
scripts/install.sh --role corporate
uv run careerbrew machine new-user --plugin <company plugin git URL or folder>
```

`machine new-user` sets the corporate role, fills their profile from `git config user.name` and `user.email`, adds their repos under `~/apps` as evidence sources (authored by their email), writes their own connectors from the plugin's `connector_defaults`, prints the `careerbrew secret set` lines for them to type, and ends with `corporate readiness: N/N`. Agents run it through `/careerbrew-corporate`.

The plugin holds only shared content: ladders, rules, policy, sensitive terms, and where the company's tools live. Your own values (your email, login, tokens, repo paths) live in your private data; `machine ready` fails if the plugin's evidence file holds any. One exception: a plugin may carry a team roster for the live feed (`live/team.yaml`: teammates' names, chat ids, logins, and emails). It is a plaintext file, it makes the feed read those coworkers' messages, and it needs company policy approval ([plugins.md](plugins.md#live-feed-sources-and-roster)).

### An older plugin that holds your own values

A plugin made before this rule may list your repos (with your author email) and your connectors in its `evidence.yaml`. `machine new-user` would rebuild them from scratch and lose your hand-picked repos and tuned fields. Move them instead:

```bash
uv run careerbrew machine split-plugin-evidence --dry-run   # the counts it would reach; writes nothing
uv run careerbrew machine split-plugin-evidence
git -C ~/careerbrew-plugins/careerbrew-<org> commit -am "chore: per-person evidence moved to private files"
```

- Per-person entries (any `email`, `user`, or `authors` field, or a home-folder `path`) move to your private `work/sources.yaml` and `work/connectors.yaml` exactly as they are: a gateway `base_url` or a custom `jql` stays.
- Shared entries stay in the plugin. A `command` source stays too, because it runs inside the plugin; edit it by hand.
- It edits the plugin's own folder (from the private lock) and reinstalls the plugin from it, so `plugin verify` stays ok and `plugin refresh` does not bring the values back. Commit the change in the plugin repo.
- It prints the evidence counts before and after: per plugin file, your two files, and the sources `work collect` reads (the same number, unless an entry was listed twice).
- A plugin installed from a git URL has no folder here to edit: clone it, run `careerbrew plugin add <its folder>` once, then rerun.
- A rerun moves nothing.

Reporting careerbrew bugs from a corporate machine: the clone is pull-only, so file an issue with the `gh issue create` command that `careerbrew machine new-user` prints, only if company policy allows it, and only after `careerbrew privacy scan --file <draft>` passes on the draft text; else file it from your personal machine, or open a PR from there.
