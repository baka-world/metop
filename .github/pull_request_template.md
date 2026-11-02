## Summary

<!-- What changed and why. One paragraph. -->

## Linked issue

Closes #<!-- issue number -->

<!-- If this PR intentionally covers more than one issue, list them and explain
why they share one contract. -->

## Changes

<!-- Bullet the files/contracts touched. Call out JSON schema or CLI changes. -->

## Verification

<!-- Paste the commands you ran and the observed result. Do not write "tests pass"
without evidence. -->

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --release --locked
cargo build --release --locked
```

Smoke / manual check:

```sh
# exact command and observed output
```

## Compatibility and risk

<!-- JSON schema change? CLI behaviour change? Permission-dependent path?
Rollback plan? Write "none" if not applicable. -->

## Checklist

- [ ] The linked issue's acceptance criteria are met.
- [ ] A regression test covers the failure mode, or the PR documents why not.
- [ ] User-facing docs/help are updated.
- [ ] `CHANGELOG.md` has an `[Unreleased]` entry for user-visible changes.
- [ ] No unrelated changes or scope expansion are included.
- [ ] I ran the verification block above and included the result.
