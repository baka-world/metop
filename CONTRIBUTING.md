# Contributing to metop

metop is small and correctness-sensitive: readers trust the numbers, and
scripts trust the JSON. The workflow below keeps changes traceable from the
first report to a verified release.

## Principles

- **Issue first.** Every behaviour change, bug fix, or documentation change
  starts as a GitHub issue with reproducible evidence and acceptance criteria.
  A trivial typo or a review comment on an open PR may skip the issue.
- **One problem per issue.** Split independent defects and features; link
  related issues instead of hiding them in one large report.
- **One issue per PR is the default.** A batch PR is acceptable only when the
  changes share one contract (for example a JSON schema version) and are
  reviewed together. Name the linked issues in the PR body.
- **No silent scope expansion.** Fix the reported behaviour; file a separate
  issue for adjacent improvements.
- **Evidence before opinions.** Include commands, versions, and observed
  output. State when a claim is inferred rather than measured.
- **PSS first.** When a metric choice is ambiguous, prefer PSS/USS semantics
  over RSS sums and document the tradeoff.

## Lifecycle

| Stage | Action | Exit criterion |
|---|---|---|
| 1. Report | Open a bug or feature issue using the template | Repro/impact and acceptance criteria are present |
| 2. Triage | Add `priority: high/medium/low` and `area: ...`; assign a milestone | Issue is in a release milestone or explicitly backlogged |
| 3. Branch | `git switch -c fix/<issue>-<slug>` or `feat/<issue>-<slug>` | Branch name contains the issue number |
| 4. Implement | Small commits; keep the working tree buildable | Acceptance criteria are met locally |
| 5. Verify | Run the verification block below; add regression tests | Commands and observed output are recorded in the PR |
| 6. PR | Use the PR template; include `Closes #N` | CI is green, docs/changelog are updated |
| 7. Merge | Squash or merge only after review | Issue closes automatically on the default branch |
| 8. Release | Follow [docs/RELEASING.md](docs/RELEASING.md) | Changelog, tag, artifacts and crate agree |

Do not close an issue manually until the fix is merged and verified. If work is
deferred, remove it from the milestone and leave a comment explaining why.

## Issue format

Both issue forms ask for the same core sections:

- **Summary** — one paragraph, user-visible symptom or missing capability.
- **Environment** — metop version/commit, distribution, relevant permissions.
- **Reproduction** (bugs) or **Problem** (features) — exact commands or a
  minimal JSON fixture.
- **Expected / Actual** — observable output, not an interpretation.
- **Impact** — who is misled and how.
- **Proposal** — the smallest acceptable change; alternatives if relevant.
- **Acceptance criteria** — checkboxes that a reviewer can verify.
- **References** — source paths, related issues, prior measurements.

Titles use Conventional Commit prefixes: `fix(json): ...`, `feat(cli): ...`,
`docs: ...`, `perf(json): ...`.

## Labels and milestones

| Label | Meaning |
|---|---|
| `bug` | Incorrect or misleading behaviour |
| `enhancement` | New capability or contract extension |
| `documentation` | README, help text, workflow docs |
| `performance` | Avoidable work or latency |
| `priority: high` | Blocks a correct or scriptable release |
| `priority: medium` | Belongs in the current milestone |
| `priority: low` | Nice to have; schedule later |
| `area: json` | `--json` snapshot and `--fields` contract |
| `area: cli` | Argument parsing and command-line UX |
| `area: diff` | `--diff` comparison |
| `area: docs` | README, help, workflow |
| `area: sampling` | PSS/RSS sampling, coverage and timing |

Every issue in active work belongs to exactly one release milestone. A
milestone is named for the version it will ship in. Use the version policy in
`docs/RELEASING.md`: pre-1.0 features bump the minor version; fixes, docs and
packaging bump the patch version. A behaviour change that breaks the JSON
schema requires a schema version bump and a matching `--diff` compatibility
path.

## Verification

Run from a clean tree before opening or updating a PR:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --release --locked
cargo build --release --locked
```

Then exercise the changed surface directly:

```sh
# JSON contract
./target/release/metop --json --fields procs.pid,procs.delta_kb | jq .
./target/release/metop --json --pid $$ --fields detail,detail_error | jq .

# Diff contract
./target/release/metop --diff old.json new.json --min-delta 10M --top 20

# TUI smoke (when the change touches rendering or input)
./target/release/metop
```

For measurement claims, report the machine, process count, and the exact
command. For permission-dependent behaviour, state whether the run was root or
unprivileged. Do not rerun a check to "confirm" a user-reported failure; use
the report as ground truth and verify the fix instead.

## Definition of done

- [ ] Every acceptance criterion in the linked issue is met.
- [ ] A regression test covers the failure mode, or the PR explains why one is
      impractical and what smoke test was run instead.
- [ ] No behaviour change is hidden behind an unrelated refactor.
- [ ] User-facing docs/help are updated.
- [ ] `CHANGELOG.md` has an entry under `[Unreleased]` for user-visible changes.
- [ ] `cargo fmt`, `cargo clippy`, and the full test suite pass.
- [ ] The changed CLI/JSON surface was exercised directly.
- [ ] The issue is closed by the merge commit or the PR body.

## Reporting security or privacy issues

Do not open a public issue for a vulnerability or a data leak. Contact the
maintainer listed in `Cargo.toml` first.

## License

By contributing, you agree that your changes are licensed under the MIT
license in `LICENSE`.
