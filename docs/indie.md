# Independent mode (`/indie`)

`/indie` is `/work` for a personal machine. It runs your own projects as income bets, so agent time goes where it pays back. The goal it serves: replace a salary with income from your own software.

## Start

```bash
careerbrew config set indie_roots "~/apps,~/apps/tooling"
```

```bash
careerbrew indie run
```

Agents: `/indie`. The first run adds one `proposed` bet per git repo it finds (two folder levels deep, one bet per origin). To name the repos yourself, set `indie_repos` to a comma-separated list of repo folder names.

## A bet

A bet is one repo plus five things only you can say:

| Field | Meaning |
|---|---|
| `kind` | `product`, `sponsors`, `audience`, `consulting`, or `enabler` (tooling that makes no income itself) |
| `hypothesis` | how this repo leads to income |
| `metric` | the number that shows it works |
| `success` | the line at or past which you keep going |
| `pivot` | the line under which you change approach |

```bash
careerbrew indie bet <id> --kind product --hypothesis "…" --metric "weekly active users" --success 50 --pivot 10
```

```bash
careerbrew indie decide <id> activate --yes
```

A bet with no hypothesis gets no agent time: it becomes a question for you. Only you decide (`activate`, `persevere`, `pause`, `pivot`, `kill`); an agent's call with no `--yes` is refused. Add `--measure "<command>"` when a command can print the metric, and `--lower-is-better` when a lower number is better.

## The day

1. `careerbrew indie run --json` ranks the active bets and names who gets a session (at most `work_session_cap`, and at most one `enabler`). Each bet's next action is the first unblocked task in its repo's `TASKS.md`.
2. Each ranked bet gets one project session (`careerbrew work session open <bet>`), which delivers that task in full.
3. `careerbrew indie measure <bet>` reads the metric: `KEEP`, `WATCH`, or `PIVOT`. Two `PIVOT` readings in a row hold the bet until you decide.
4. `careerbrew indie end` records the day's agent-hours and asks per bet.
5. `careerbrew indie review` shows each week: outcomes, agent-hours per outcome, and asks per outcome.

What an agent sees: ids, states, counts, and `KEEP` / `WATCH` / `PIVOT`. A bet's hypothesis, its success and pivot lines, and each reading's value show only in your own terminal. The plan's `coverage` counts the repos, those with any next action (`with_next_action`), and those whose next action is a task (`with_task`).

Limits to know:

- A week's trend is its total agent-hours (and asks) over its total outcomes. A week with a failed agent-time read shows no hours, never a lower number.
- An outcome counts for the week its commit landed, even when no hours were recorded for that bet. Record hours every day (`careerbrew indie end`) or the cost per outcome reads too low.
- A measure command that exits with an error records no reading.

## How bets are ranked

- An **outcome** is a merged `feat`, `fix`, or `perf` commit in the bet's repo. Upkeep does not count.
- A bet's score is its outcomes per agent-hour plus a bonus for bets tried less (UCB1, Auer et al. 2002). An untried bet goes first, so each bet gets one turn before the numbers decide.
- A held or paused bet gets no session.
- Agent-hours come from the session clock. To read them from another tool, set `indie_agent_time_command` to a command with `{project}` and `{days}` that prints JSON with `toolMs` (milliseconds).

In a seeded 8-week simulation over 200 seeds, this ranking needs 38.5% fewer agent-hours per outcome than round-robin by weeks 7 to 8 (10th percentile 29.2%, worst seed 13.7%). `tests/unit/test_indie.py` pins it.

## Your income goal

```bash
careerbrew indie goal --monthly <amount> --by 2027-12-31
```

```bash
careerbrew indie income --month 2026-11 --amount <amount>
```

You type both. Every command prints only a percentage of the target, so an agent never sees an amount.

## `/indie` and `/work` improve each other

The two runners have two personalities. `/indie` is the founder: bold, it counts outcomes per agent-hour. `/work` is the operator: careful, it counts failed and blocked steps. Each keeps numbered lessons in its skill's `LESSONS.md`.

- `careerbrew peer status --for indie` lists the `/work` lessons that `/indie` has no verdict on, and the `/work` source commits since its last review. `--for work` does the reverse.
- `careerbrew peer experiment --side indie --start "<change>" --metric hours_per_outcome --lesson L-work-4` starts a trial. One trial per side at a time.
- After 3 days or more, `careerbrew peer experiment --side indie --close` compares the metric with its 14-day baseline: `KEEP` or `REVERT` needs a 10% move, less is `NEUTRAL`, no number is `NO-DATA`. It prints the line to add to `LESSONS.md`.

Lessons hold process rules only. A corporate machine never pushes, so a `/work` lesson learned there reaches the repo when you add its line from a personal machine.

## Privacy

Bets, goal, income, readings, and day rows live encrypted in `private/indie/`; trials live in `private/peer/`. `careerbrew indie` refuses a corporate-role machine. A bet's measure command and the agent-time command run on your machine with no shell; careerbrew reads one number from their output. Details: [privacy.md](privacy.md).
