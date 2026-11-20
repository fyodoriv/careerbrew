# Synthetic artifact-proof fixture

This is browser test tooling for the frozen
prospective experiment.
It is not a Careerbrew feature or a real-service integration test.

`prepare.mjs` fills static UI templates in a fresh empty directory. It starts no
server, browser or agent. The maintained Python standard-library HTTP server
serves that directory. There is no new backend implementation or dependency.

Migration scope follows the canonical Rust code inventory: retain the client
HTML/CSS/JS as UI. Assign the preparer's owner-side file setup, key/hash receipt
and generated source-write shell tool to `rust-27`; its fixture folder does not
exempt those helpers. The maintained `python -m http.server` invocation is a
local synthetic-fixture dependency only. It supplies no production Rust
qualification and must leave the final Python-free test/maintenance path.

The owner selects one of the six registered cases. The participant gets an opaque
run path, neutral `input.json`, the supplied house skill, and the rendered page.
Keep fixture modes, labels, keys and the grader rubric outside its read scope.
The UI source may reveal ordinary behavior. It does not contain answer labels.

The provider file is separate from browser-page state. Its registered update
command is a tool, executed only on participant request. Record its invocation,
nonce, write time and hash. The ordinary page polls its actual HTTP endpoint.
Only that source file and browser-context review storage may change during a run.
Hash the prepared HTML, JS and other static assets before and after execution.

Use the plan's exact setup, source-write and `heavy-run` browser commands. Retain
Playwright trace/HAR, downloads, screenshots and server logs outside git. The
functional grader checks actual effects; the reader gets only the finished
artifact. Never substitute a participant's account of tool use for these records.

The fixture's UI budget is 16 px body text, 44 px controls, no sideways scroll,
light/dark contrast, reduced motion, and one click for export, review or help.
Before participant runs, check the prepared page on phone and desktop in both
themes with axe and browser measurements. This comfort preflight does not count
as an instruction baseline or revised-skill result.

Each server belongs to its recorded process and scratch directory. Each browser
run uses a fresh context and closes it in `finally`. Stop only the owned server
after retaining evidence. Closing the context deletes the fixture review choice.
Never use owner data or browser profiles. Never commit runtime evidence.

## Pure source preparation

`source-preflight-v3.mjs` prepares a source graph from the explicit supplied
buffers. It reuses the closed admission helper and the isolated maintained
parsers. Its three developer exports are synchronous. Graph buffers belong to
one delivery invocation and remain private; `copyPayload` checks their hashes
and returns a fresh copy. Receipts expose only fixed metadata and bindings.
The six injected owner decisions are not process-cleanup evidence.

The registered source harness has 57 original rows, 70 recipes and six owner
decisions. Both fresh 133-case runs passed with identical semantic receipts.
Before it calls a parser, it requires an ignored root execution release bound
to the reviewed source hashes and installed-source census. Source preparation
and a missing-module RED alone are not positive parser results. Native workers,
headers, browsers, the 57 native transport cases and reader cohorts remain held.
The optional tools and doctor are described in [INSTALL](../../../INSTALL.md#4c-optional-artifact-source-developer-tools).

The permanent shared-stylesheet depth regression is a separate two-case suite.
After selecting the declared optional Node 24 tools, run it explicitly:

```sh
heavy-run node --test --test-concurrency=1 --test-timeout=30000 evals/fixtures/artifact-proof/source-preflight-v3-depth.test.mjs
```

It retains the registered shallow-first/deep-first pair without changing the
133-case denominator or requiring the original harness's local release file.
