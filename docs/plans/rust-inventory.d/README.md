# Code inventory fragments

Add new code inventory rows here, not in `../2026-11-01-rust-code-inventory.csv`.

- One file per step or pull request: `<name>.csv`, for example `rust-12-config.csv`.
- Same columns as the main inventory: `path,disposition,task,reason`.
- The task matches the disposition: `keep-ui` has an empty task, `keep-native`
  uses `rust-00`, and `port` uses its two-digit migration step.
- No header line. A fragment that starts with the header is refused.
- `cargo xtask migration inventory --check` reads the main inventory, then every
  staged `*.csv` here in sorted name order, and checks the combined rows.
- A path listed twice, in any two files, is refused.

Why: every step PR used to append to the one shared CSV, so each merge made the
other open PRs conflict. Separate files do not conflict.
