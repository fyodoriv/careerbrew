# Pedagogy for deep learning runs

Each rule names the source it rests on. Re-check a source before you change a rule.
`careerbrew learn` enforces what a machine can check (citations, cycles, the recall cap,
misconception tags, mastery, scheduling); the agent follows the rest.

## 1. Verify each question, tag each distractor

- Answer each question from the study notes only. Keep it only if the notes support exactly one answer and rule out every distractor; otherwise rewrite it. Do the same for every question you write live. Source: Dhuliawala et al. 2023, Chain-of-Verification.
- Each wrong option targets one named misconception (`misconceptions:` in `concepts.yaml`, one per wrong option; `learn concepts` checks the count). Never show the tags. Build them from errors the sources warn about and from the user's past misses. Source: Hestenes et al. 1992, Force Concept Inventory.
- On a miss say: "You may be thinking {misconception}. The notes say {fact} ({note})."

## 2. Open recall is the default

- Recall beats recognition, so quiz rounds ask "explain", "why", or "what happens when" in a plain message and wait for prose. Source: Roediger & Karpicke 2006, retrieval practice.
- Before you ask, write 2 to 4 hidden rubric points, each one stated in a cited note. Grade by meaning, not keywords. Show the score (2/3) and quote the note for each missed point.
- `correct` only when every point is met; `partial` when some are. At most 30% of a deck's questions may be pure recall (`remember`).

## 3. Predict, read, explain

- Start each new section with a pretest: ask the user to predict before you teach. Missed pretest questions still raise later recall. `learn next` schedules one per untouched section and does not score it. Source: Richland, Kornell & Kao 2009, "The pretesting effect".
- While teaching, stop after each short piece and ask the user to explain why it holds in their own words. Source: Chi et al. 1994, self-explanation effect.
- Hints come in tiers: the place to look, then the concept's name, then a worked example of a similar case, then the answer. Worked examples help novices most and fade as skill grows. Sources: Renkl & Atkinson 2003 (fading worked examples); Kalyuga et al. 2003 (expertise reversal).

## 4. Bloom levels and the ladder

- Tag every question with one level: `remember` · `understand` · `apply` · `analyze` · `evaluate` · `create`. Source: Krathwohl 2002, "A Revision of Bloom's Taxonomy: An Overview", *Theory Into Practice* 41(4), 212–218.
- Applied tasks on the real repo (locate the owner, where to add, trace the bug, blast radius) are `apply` or `analyze`.
- `learn next` asks one level above the best confident correct answer so far; reviews and retests ask at that best level.

## 5. Concepts and prerequisites

`concepts.yaml` shape:

```yaml
concepts:
  - id: queue
    section: 01-core
    title: How a job enters the queue
    prereqs: []
    cites: [repo:svc/src/queue.py, note:01-core/queue.md]
    questions:
      - {bloom: understand, prompt: "Why does enqueue return before the job runs?", answer: "…"}
      - bloom: apply
        prompt: "A job is enqueued twice. What happens?"
        answer: "…"
        options: ["runs once", "runs twice", "raises"]
        misconceptions: ["dedupe happens at enqueue", "enqueue is synchronous"]
```

- A citation is a note (`note:<path>`), a staged source key (`repo:<name>`, `doc:<label>`), or a file in a staged repo (`repo:<name>/<path>`) that exists.
- Each prerequisite edge is a real dependency the sources state. No cycles: `learn concepts` refuses them.
- Objectives use action verbs (explain, compare, choose, debug, locate), not "understand X".
- `learn next` teaches a concept only after its prerequisites are mastered, weakest section first.

## 6. Confidence and calibration

Ask Low / Medium / High after the answer and before the result.

| Confidence | Result | Meaning | Action |
|---|---|---|---|
| High | Wrong | Misconception | Name it, quote the note, drill it first. |
| Low | Correct | Fragile | Does not count toward mastery. |
| High | Correct | Known | Counts. |
| Low / Medium | Wrong | Gap | Teach again. |

Source: Butterfield & Metcalfe 2001, hypercorrection of high-confidence errors. `learn status` reports the Brier score (squared gap between stated confidence and the result).

## 7. Mastery, spacing, retests

- Mastered: two correct answers that were not low-confidence, one at `apply` or higher. A Bayesian Knowledge Tracing model was tried against this rule in simulation and lost (pinned in tests/unit/test_learn.py).
- Every taught concept is a card in the project's own FSRS deck, so reviews come back across days. Sources: Cepeda et al. 2006 (distributed practice); Ye et al. 2022, FSRS.
- A mastered concept is retested at +1 and +7 days in new words. Only a delayed test shows learning; performance in the session does not. Source: Soderstrom & Bjork 2015, "Learning versus performance".
- Once each section's core is mastered, interleave sections. Source: Rohrer & Taylor 2007.
