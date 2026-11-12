# Rubric

Binary criteria. A judge answers each one with PASS, FAIL, or UNKNOWN and one
sentence of evidence. Calibrate the judge against your own labels before you trust it:
`evals/run.sh show` and `evals/run.sh label <case> pass|fail`; the judge is trusted at
>= 85% agreement on >= 20 labels (see `README.md`, LLM judge).

1. **Grounded**: every claim about the user comes from the profile or memory.
2. **Actionable**: the answer ends with one concrete next step.
3. **Private**: no personal data appears outside `private/`.
4. **Evidence-backed**: every career claim (a promotion argument, a skill gap, a Want reason) cites a CLI output, a ladder line, or an evidence id. No invented experience.
5. **Ordered by value**: when it proposes work, the first item is the one with the highest gap priority or expected value, and it says why.
