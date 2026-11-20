---
name: careerbrew-bet-options
description: Proposes five income-bet options for one personal project when the owner cannot name its hypothesis, and shows only options that five independent judges all pass. Each option is a full bet (kind, hypothesis, metric, success line, pivot line, how to read the metric). Uses public facts about the repo only. The owner picks, edits, or rejects; the skill never saves or activates a bet by itself. Use when the owner says "I'm not sure" to a bet question from `/indie`, "give me options for this bet", "how could this project earn", or "help me pick a hypothesis". Don't use to run the indie day (use careerbrew-indie) or when the owner already gave a hypothesis.
---

# Bet options

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

`/indie` gives a project no agent time until its bet has a hypothesis. That text is the owner's. When the owner is not sure, this skill gives them five judged options to choose from. The choice stays with the owner.

## Steps

1. **Facts first, public only.** Read the repo's README and vision, and take today's public numbers with commands: stars and forks (`gh repo view`), package downloads (the registry's public count), distinct outside issue authors. Write each number with its command and date. Never read `private/`, and never use the owner's income, salary, or employer.
2. **Draft 8 candidates.** Cover at least 3 of the kinds (`product`, `sponsors`, `audience`, `consulting`, `enabler`). Each candidate has:
   - **kind** and a one-line **hypothesis**: who pays or what it unlocks, and why this project.
   - **metric**: one number an agent or the owner can read today. Start from today's value.
   - **measure**: the exact command that prints the metric, or "owner reads it from <place>".
   - **success** and **pivot**: two numbers with a date, at most 8 weeks out. Pivot is under success.
   - **first task**: the first piece of work the bet needs.
   - **traffic** and **first reading**: where the people come from (a form with no stated traffic source reads zero whether or not demand exists), and the date of the first pivot reading, so a metric that starts at zero does not hold the bet in week 1.
   - The measure command must count exactly what the metric text says. A feeder bet names what it feeds and how that link is counted.
3. **Five judges, fresh context each.** Start five subagents at once. Give each the facts, all candidates, and one role. Each judge returns `PASS` or `FAIL` per candidate with one reason, and the fix that would make a fail pass.
   - **Demand judge**: is there public evidence that someone wants this and pays for things like it? Cites a source per claim.
   - **Income judge**: can this path reach a full-time income for one person? Shows the arithmetic with public prices; no owner amounts.
   - **Measure judge**: is the metric readable today, hard to game, and are success and pivot numeric, dated, and reachable from today's value?
   - **Solo judge**: can one person with agents get the first reading in 8 weeks, without a team, an employer's resources, or a conflict with the owner's rules (vendors, privacy, employer)?
   - **Skeptic**: the strongest reason this fails. `FAIL` when that reason has no answer.
4. **Fix once, then cut.** Revise each failed candidate with the judges' fixes and send the revised set back to all five judges, as the final round for those candidates. A candidate that still fails any judge is cut.
5. **Second batch for empty slots.** If fewer than five passed, draft new candidates (not the cut ones again) that answer the judges' failure reasons, and send them to five fresh judges once. At most two batches in all.
6. **Show five.** Rank the candidates that passed all five judges by the Income judge's number, then by time to first reading. Show the top five in one table: kind, hypothesis, metric (today → success / pivot, date), first task, and the Skeptic's reason in one line. If fewer than five passed, show those and say how many were cut and why. Never pad the list with a candidate that failed a judge.
7. **The owner picks.** Ask once: pick one, edit one, or reject all. Say which you would pick and why in one line. After the owner's answer, save it with `careerbrew indie bet <id> --kind … --hypothesis … --metric … --success … --pivot … [--measure …]`, and run `careerbrew indie decide <id> activate --yes` only after the owner says yes to activation.

## Rules

- **The owner's words win.** An option is a draft until the owner picks it. Never save or activate an option the owner did not pick.
- **No invented facts.** A number with no command or source is not a fact; mark it "unknown".
- **No amounts of the owner's.** Income arithmetic uses public prices and counts of customers, never the owner's target.
- **Judges are independent.** A judge sees the facts and the candidates, not the other judges' verdicts or your favorite.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

After each run, add one line to `skills/careerbrew-bet-options/LESSONS.md`: which judge cut the most candidates and why, and whether the owner picked, edited, or rejected. If the behavior should change, update this skill and add a case in `evals/cases/`.
