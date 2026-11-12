# Darwin leaf units: prospective calibration

This is phase B of the accounting plan.
Commit this complete prediction before implementation. The base is the locally
qualified accounting integration, `89a7155f`, tree `09637fdd`. Its full gate and
normal hooks passed. That result does not calibrate resource units.

## Goal, question and prediction

Goal: establish a version-bound unit interpretation for the fixed native leaf.
Question: does raw Darwin wait4 maximum RSS use bytes, as a separate Mach API
with a byte contract predicts? The metric is the number of six fixed fresh
invocations that uniquely match one interpretation under the limits below.

Prediction: byte interpretation matches 6/6; KiB interpretation matches 0/6.
Keep only at 6/6 unique matches, every ownership and oracle check passing, and
the normal full gate exiting zero. Any ambiguity, missing outcome or failed
bound leaves units unavailable. Do not replace samples, change the schedule,
widen a bound or import a favorable subset. Preserve every failed attempt.

This experiment adds no admitted performance-sample API. The actual resource
value T comes only from the existing owner's exact-PID wait4. The Mach value R
is a fixture-only unit oracle. Both APIs use the kernel memory ledger; this is
not independent physical-memory ground truth.

## Prospective design replacement and trust

An uncommitted proposal used three NPROC denial controls and a pre_exec callback.
No guard, leaf, calibration or resource probe ran. Source review did not prove
the installed limit wrapper's post-fork safety. Do not implement that callback,
change parent limits, substitute a private syscall, or label it safe.

The committed phase-A obligation is a fixed trusted leaf without descendants.
It does not require confinement of arbitrary programs. Replace the three
unexecuted denial controls with source and binary closure admission. Keep the
original six samples and all numerical criteria unchanged.

Explicit trust: the pinned installed kernel ABI, dyld and libSystem startup.
Do not pretend system initialization is absent. The leaf itself is no_std,
no_main, with a C main entry and panic=abort. It has no own constructors,
allocator, threads, process creation, exec, dlopen, callbacks or user code.
Its finite paths perform argument/descriptor checks, memory work, the Mach
query, bounded record writes and _exit. A panic handler exits with status 101.

Audit the future complete source and Mach-O before any actuals. Cover imported
images, load commands, initializers, indirect calls, compiler helpers, panic
and error paths. An unexplained path blocks release. This establishes only
the reviewed fixed fixture's call closure, not a sandbox or a guarantee for
another executable. No process census or sampling maximum closes that gap.

Keep all real actor imports, code, C entry and panic handler behind
`cfg(all(target_os="macos", panic="abort"))`, including matching cfg_attr for
no_std/no_main. Other builds contain only an ordinary std exit78 refusal stub.
This lets normal all-target checks build without changing Cargo profiles.
The stub performs no resource invocation and cannot qualify this experiment.
Check the real abort branch separately, then retain a 0700 byte-pinned copy of
that compiled binary. The harness must use only that frozen copy. It must
never build a leaf, search target/debug/examples, or fall back to a stub.

## Exact leaf and lifetime

The parent selects one absolute, byte-pinned, owner-only executable. Reject
setuid/setgid mode, changed ownership, root or mismatched real/effective
credentials. Clear the child environment explicitly; the shared owner does
not do this for callers. Pass only a fixed S or L mode and the owned descriptor
number described below. Permit no target, path, command, environment override
or payload option. Bind the actual argv and environment keys to each receipt.

S maps 16 MiB; L maps 128 MiB. Use anonymous private read/write memory. Require
page size 4096 or 16384 and exact alignment. Initialize the small record buffer
before work. Write every u64 with volatile stores:

`x_i = (i * 0x9e3779b97f4a7c15 + 0xd1b54a32d192ed03) mod 2^64`.

Read every word twice with volatile loads. Each sum must equal the fixed
wrapping formula `a*(N/2)*(N-1)+b*N`, with `N=M/8` (both N values are even).
Expected sums are `0x3fba67b5df100000` for S and `0x941c6daef8800000` for L.
No adaptive iteration or time-based hold is allowed. Retain the mapping until
_exit. Touching every page is not a guarantee of residency under pressure.

Query task_info with MACH_TASK_BASIC_INFO using the pinned libc ABI. Require
success and the exact returned count. Copy resident_size_max by value without
an unaligned reference. After the query, perform only fixed record encoding,
bounded write completion and _exit. Add no allocation or further workload.

Success status is 0. Fixed refusals are 64 arguments, 65 descriptor/identity,
66 credentials/platform/page, 67 mapping, 68 checksum, 69 Mach/count, and 70
record write. Any other exit, signal, deadline or ownership failure is invalid.

## Owned oracle descriptor

Replace the uncommitted literal-FD3 idea with the descriptor returned by opening
the invocation's own file. Never reserve or overwrite a foreign descriptor.
The parent creates a read/write file with create_new and mode 0600 in a fresh
0700 directory. Keep its live File object throughout flag changes and reading.
Require its descriptor to be at least 3. Pass its strict decimal number as
bounded metadata, not an arbitrary input-file interface.

Before spawning, save exact descriptor flags. Temporarily clear CLOEXEC only
on this File. Restore the exact flags with a checked result on every return
and error path before another parent spawn. A guard must own or borrow the File,
not retain a stale raw number. If restoration fails, close only the owned File
and refuse qualification. No dup2 onto a parent or foreign descriptor and no
pre_exec callback are allowed.

The target has one child-creating test function and six serial invocations.
No other parent process spawn is allowed while this descriptor is inheritable.
Verify this condition in the future source and test-binary review. Pure record
checks may not create children. Review the selected std spawn route and its
file actions before relying on inheritance; unexplained closure is unavailable.

The leaf accepts exactly argc 3. Parse at most ten ASCII digits for a positive
descriptor in 3..INT_MAX; reject signs, whitespace, suffixes and overflow.
Use fstat to require a regular, owner-matching, initially empty 0600 file.
Include fstat, getpid and credential helpers in the reviewed finite import
closure. No pathname is passed to or opened by the leaf.

One record is exactly 96 bytes, all integer fields little-endian:

| Offset | Field |
|---|---|
| 0..8 | ASCII magic CBRSSU01 |
| 8..12 | version 1, u32 |
| 12..16 | actual PID, u32 |
| 16..20 | mode 0=S or 1=L, u32 |
| 20..24 | inherited owned descriptor, u32 |
| 24..32 | M, u64 |
| 32..40 | page size, u64 |
| 40..48 | first checksum, u64 |
| 48..56 | second checksum, u64 |
| 56..64 | Mach resident maximum R, u64 |
| 64..72 | device identity, u64 |
| 72..80 | inode identity, u64 |
| 80..84 | effective UID, u32 |
| 84..88 | native file mode, u32 |
| 88..96 | reserved zero, u64 |

Bind device/inode conversion to the reviewed native ABI. The parent reads the
same owned File after terminal reap and flag restoration. Require exact length,
magic/version/reserved, PID, mode/M, descriptor/device/inode/UID/file mode, page
size and both checksums. Refuse incomplete or extra bytes. Bound writes to 32
attempts, retry only interruption, and fail on zero progress or another error.
Keep stdout/stderr separate. Never derive T from any child-reported field.

Pure record controls must cover valid S/L; every bad identity or checksum;
lengths 95/97; bad magic/version/reserved; invalid page; negative raw RSS;
KiB multiplication overflow; and nonzero/signal/deadline/missing raw usage.
All invalid records and outcomes must be refused. Synthetic checks are not
native samples and cannot substitute for the six actual invocations.

## Fixed schedule, bounds and pivot

Run exactly S,L,S,L,S,L, each in a fresh process with a fresh record. Use the
existing exact-child owner and nonconsuming terminal observation, with a fixed
30-second per-child hang guard. Require one terminal consumption, valid exit0,
released ownership and exact owned-process absence evidence. Keep raw typed
usage, outcomes and failures. Unresolved cleanup stops further execution and
leaves the experiment incomplete; it does not permit a replacement attempt.

For every sample require `M <= R <= M + 32 MiB`.

- Byte interpretation matches iff `R <= T <= R + 8 MiB`.
- KiB interpretation matches iff `R <= 1024*T <= R + 8 MiB`.
- Use checked arithmetic. Exactly the same interpretation must uniquely match
  all six invocations.
- For each adjacent L-S pair, both R and selected T differences must be
  80..144 MiB inclusive, and both L/S ratios must be at least 2.

The 112 MiB payload contrast, 32 MiB overhead variation and 8 MiB terminal
allowance were selected before samples in the original proposal. They are
keep/refuse bands, not error guarantees for other workloads. Compression,
bootstrap history or tail work can make the design fail. Preserve that failure
and revisit the method prospectively; never widen a band from observed values.

## Commands, sequence and allowlist

Only this document belongs in the prediction commit. Run the normal docs fast
gate through heavy-run and normal hooks first. No code before that commit.

After a separate source release, add only the fixed native example
`xtask/examples/wait4_leaf.rs`, private target
`xtask/tests/wait4_unit_calibration.rs`, exact ignore/inventory entries and
privacy/agent documentation. Reuse the existing owner, Rust 1.99.1 and libc
0.2.191. Add no dependency, Cargo/lock change, public action or normalizer API.
Platforms outside the reviewed Darwin target report unavailable; do not count
a platform refusal as calibration success. Preserve existing 33/32 and DATA13.

Register the initial missing-target compile RED as preparation evidence only.
Source review precedes compile-only admission. The compile command is:

```sh
heavy-run cargo rustc --locked --offline -p careerbrew-xtask --example wait4_leaf -- -C panic=abort
```

Strictly check the same real branch with:

```sh
heavy-run cargo clippy --locked --offline -p careerbrew-xtask --example wait4_leaf -- -C panic=abort -D warnings
```

Normal all-target checks must also build the refusal stub. Register only the
top-level actual function `fixed_darwin_leaf_units` with an ignore attribute
explaining that it requires the reviewed exclusive experiment. Run pure record
controls normally. Ignore no existing accounting, CLI/HTTP/deadline or DATA
control. Normal full gates must report one ignored actual experiment and zero
calibration invocations; that is not calibration success.

Freeze source/index, SDK/ABI/toolchain and leaf/test binary hashes. Review the
actual Mach-O closure and owned-FD path, not only this plan. Only after that
separate release may this one actual command run:

```sh
heavy-run --exclusive cargo test --locked --offline -p careerbrew-xtask --test wait4_unit_calibration -- --ignored --exact fixed_darwin_leaf_units
```

Require exactly one selected actual test and all six native outcomes. Zero
selected tests is failure. Before spawning, validate the later root release,
every source/binary pin and the fixed frozen-copy location. Require a fresh
receipt with no overwrite, so a second invocation cannot resample silently.

Use an inside-slot 3 GiB free-space preflight. Keep fresh bounded 0600 receipts
in invocation-owned 0700 directories. Bind OS product/build, full Darwin kernel,
architecture/page size, SDK headers/compiler/libc, loader/runtime identities,
argv, binaries and all source hashes. SDK exports do not prove an installed
implementation revision. No old host version is a substitute for this binding.

After actual review, run `heavy-run env CHECK_EVALS=1 scripts/check.sh` before
delivery. It must preserve the existing 33 accounting groups, 32 original
CLI/HTTP/deadline tests and DATA13 controls. No resource threshold is relaxed.

Later 30 paired performance trials, CPU floors/noise/order, slow and retained
memory negatives remain separately held. This six-sample check grants no
whole-tree CPU/RSS claim, Linux unit result, production switch or Rust00 closure.

## Anchors and source basis

Basili, Caldiera and Rombach (1994), Goal Question Metric; Munafò et al. (2017),
preregistration. The fixed observable keep/pivot rules precede every sample.
The phase-A plan remains authoritative
for ownership, preservation and unsupported broader scope.

Mach byte ABI: [Apple task_info.h](https://github.com/apple-oss-distributions/xnu/blob/xnu-11417.121.6/osfmk/mach/task_info.h).
This is a contract reference, not an assertion that its XNU version is installed.
Pinned Rust spawn source:
[58cf8f9c3 std unix.rs](https://github.com/rust-lang/rust/blob/58cf8f9c3/library/std/src/sys/process/unix/unix.rs).
The installed reviewed copy has SHA256
`1c0f874f1835e5befa6e8bdd65450903f475248b525d3b7b5e1ce861311a21d0`.
Its inspected macOS spawn route has no CLOEXEC_DEFAULT or closefrom action;
stdio duplication destinations are 0, 1 and 2. Recheck the selected route and
actual binary before relying on these facts in the future fixture.
