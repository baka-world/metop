# Releasing mtop

mtop uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html) and
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The version in
`Cargo.toml` is the single source of truth: the release tag must be exactly
`v` + that version, and CI enforces it.

## Version policy

| Change | Version |
|---|---|
| Backwards-incompatible behaviour, or new functionality | `0.MINOR.0` |
| Bug fix, docs, packaging, dependency update | `0.MINOR.PATCH` |
| Stable public API (post 1.0) | `MAJOR.MINOR.PATCH` |

Pre-1.0, additive features bump the minor version; this keeps patch releases
strictly for fixes. Publish a release only when the changelog has a matching
entry.

## Release checklist

1. Move the relevant `## [Unreleased]` entries in `CHANGELOG.md` into a new
   `## [X.Y.Z] - YYYY-MM-DD` section and update the compare links.
2. Bump `version` in `Cargo.toml`, then run `cargo check` so `Cargo.lock`
   records the new version. Commit this together with the changelog as
   `Release vX.Y.Z`.
3. Verify locally:

   ```sh
   cargo fmt --check
   cargo clippy --all-targets --locked -- -D warnings
   cargo test --locked
   cargo build --release --locked
   ./target/release/mtop --version   # must print X.Y.Z
   ```

4. Push `main` and wait for the CI workflow to pass before tagging.
5. Create an annotated tag and push it:

   ```sh
   git tag -a vX.Y.Z -m "mtop vX.Y.Z"
   git push origin vX.Y.Z
   ```

   The release workflow checks `scripts/check-release-version.sh vX.Y.Z`, builds
   amd64/aarch64 assets, publishes the GitHub release, and uploads
   `SHA256SUMS-*`.
6. Verify the published artifacts (do not trust the workflow alone):

   ```sh
   gh release view vX.Y.Z
   gh release download vX.Y.Z --pattern 'SHA256SUMS-*'
   sha256sum -c SHA256SUMS-x86_64-unknown-linux-musl
   ```

   Set the release notes to the changelog section for `X.Y.Z`.

## Tag rules

- Published tags are immutable. Never delete, move, or force-push a tag that
  has a GitHub release; fix mistakes with a new patch release.
- Never tag a commit whose `Cargo.toml` version differs from the tag; the
  release workflow fails early in that case.
- Do not bundle a version bump into an unrelated feature commit. The release
  commit (`Release vX.Y.Z`) contains only the version bump and changelog
  promotion, so the diff between tags is exactly the released code.

## crates.io

The GitHub project is `mtop`, but the crates.io name `mtop` is already taken
(memcached top). The published crate is therefore named **`metop`**:

- `[lib] name = "mtop"` keeps the Rust library name `mtop`;
- `[[bin]] name = "mtop"` keeps `cargo install metop` producing the `mtop`
  binary;
- `[package.metadata.deb].name` / `[package.metadata.generate-rpm].name` keep
  the GitHub release packages named `mtop`.

Publishing requires a crates.io token (`cargo login`) and a clean tree:

```sh
cargo package --list --locked
cargo publish --dry-run --locked
cargo publish --locked
```

The version must match `Cargo.toml`; do not move the GitHub tag after a crate
is published. A crate version is immutable once uploaded.
