# Third-party licences

careerbrew is under the MIT licence. It installs its dependencies from their
registries through the lock files. The repository holds no copy of their
source.

`scripts/public-readiness.sh` (surface S10) reads the licence of every locked
package. Most are permissive (MIT, Apache-2.0, BSD, ISC). Three are not, and
`config/public-readiness-allow.txt` allows each one by name and licence:

| Package | Ecosystem | Licence | Why it is acceptable |
|---|---|---|---|
| `recurring-ical-events` | Python | LGPL-3.0-or-later | An unmodified dependency, imported and not bundled. |
| `x-wr-timezone` | Python | LGPL-3.0-or-later | An unmodified dependency of `recurring-ical-events`, not bundled. |
| `fernet` 0.2.2 | Rust | MPL-2.0 | An unmodified crate. MPL-2.0 is file-level copyleft: static linking is allowed, and only changes to its own files must be shared. |

If you change one of these packages or copy its source into this repository,
its licence applies to that code: remove the allow line and decide again.
If you ship a binary that links `fernet`, say where its source is
(crates.io). S10 fails again when one of them changes its licence.

One more package has an allow line because its metadata does not name one
licence. `python-dateutil` says "BSD License" and "Apache Software License".
Its licence file gives Apache-2.0 for new work and the 3-clause BSD licence
for old work. Both are permissive. S10 does not guess which BSD licence a
package means, so the line records what the licence file says.
