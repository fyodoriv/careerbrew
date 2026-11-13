# Optional source-preflight developer tools

This private package pins parse5 8.1.0, CSS Tree 3.2.1 and data-urls 8.0.0.
Use Node 24.15+ below 25 and npm 12.2.1. It does not change the web app's
Node 20 setup. The reviewed lock is the dependency source of truth.

Keep `node_modules` local and untracked. Resolve only through this package;
do not use a web-package or hidden-cache fallback. The opt-in doctor reads
manifests and lock identities without importing a parser. It is readiness
metadata, not package security, source qualification or native image proof.

The 133-case qualification harness uses the separately reviewed Node 24.21.0
identity. Its root release binds all source and installed-package hashes before
qualification runs. No browser, image decoder, Sharp or resource worker belongs here.

From the repository root, the separate permanent depth regression is opt-in:

```sh
heavy-run node --test --test-concurrency=1 --test-timeout=30000 evals/fixtures/artifact-proof/source-preflight-v3-depth.test.mjs
```

Use the declared Node 24 installation. This runs the same two source-only
cases registered in `fb727f1a`; it does not import the 133-case harness, need
its local capability file, or qualify native delivery.
