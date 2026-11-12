# Evals

Evals prove the assistant still does its job after a prompt, skill, or model change.

- `cases/`: one YAML file per skill, holding one or more cases as YAML
  documents (`---`), synthetic data only. Each case has the input, the
  expected outcome, and whether the assistant should act (`should: act`) or
  hold back (`should: refrain`). Write cases from real failures first.
  The case loader rejects duplicate mapping keys, including nested keys, with
  the source file and line. Valid YAML merge overrides keep safe-loader behavior.
  Separate cases with `---`; repeated fields in
  different documents remain valid. Run `uv run pytest -q
  tests/unit/test_eval_case_loading.py` to check this boundary without a model.
- `rubric.md`: binary pass/fail criteria. The same rubric drives the
  assistant's own feedback and the judge, so feedback quality is under test too.
- Graders: prefer code checks (regex, schema). Use an LLM judge only for open
  quality, one criterion per call, on the local route in `config/models.yaml`.
- Two suites: `regression` (must stay near 100%; run on every prompt or skill
  change) and `capability` (new, harder cases; may fail).
- Runner: `evals/run.sh`. `CHECK_EVALS=1 scripts/check.sh` runs it and fails
  when it is missing. Wire it to `promptfoo eval` for an LLM app, or to your
  coding agent's skill-eval runner for a skills-only assistant.

## LLM judge (`EVALS_LLM=1`)

`EVALS_LLM=1 evals/run.sh` also runs every `input:` + `expect:` case on the
routes in `config/models.yaml` (`judge.answer_route` answers with the case's
skill as its prompt; `judge.route`, the local one, grades each criterion in its
own call). Code: `src/careerbrew/judge.py`.

- **Trust**: judged failures fail the run only once the judge agrees with your
  labels on at least 85% of at least 20 labeled cases. Until then they are
  listed with `[judge not trusted yet]`. Below 70%, switch to code graders.
- **Label** (you, the owner): `evals/run.sh show` prints the next unlabeled
  case, its criteria, and its answer. Read it, then run
  `evals/run.sh label <case> pass` (it meets every criterion) or `... fail`.
  `evals/run.sh agreement` prints the agreement report only. Commit
  `evals/labels.jsonl`.
- **Labels file** `evals/labels.jsonl`, one JSON object per line: `case`,
  `label` (`pass` or `fail`), `answer` (the text you read), `answer_sha`,
  `expect_sha` (hashes of the answer and the criteria), and `labeled`
  (the date). An edited answer fails the evals; fix it with
  `evals/run.sh show <case>`, then `evals/run.sh label <case> pass|fail`,
  which replaces that line. A case whose criteria changed has a stale label,
  left out until you label it again.
- **Cache**: model replies are kept in `.metrics/evals/`, keyed by a hash of
  their inputs, so a rerun calls no model for an unchanged case. The first run
  of all cases on a busy machine can take a long time. HTTP responses must report
  `finish_reason: stop`; truncated responses and responses without a completion
  reason fail before grading or caching. Older text-only cache entries are not
  reused because they do not prove completion.
- **Environment**: `EVALS_LABELS` (labels file), `EVALS_CACHE` (cache folder),
  and `EVALS_LLM_URL` (another OpenAI-compatible server on this machine;
  tests point it at `tests/e2e/fake-judge.py`). Cache and label paths must be
  outside the careerbrew data root, including its plaintext output folders.
  Move any existing eval files outside the root before setting these paths.
  Case and skill files must also resolve within their own repository folders.
  The designated source directories must be real directories. A file symlink
  is allowed when its target stays in that file's repository folder.
- A missing model is an error that names the fix (for example
  `ollama pull <model>`, or a model you have for the `local` route).
