#!/bin/sh
# Fail unless the release tag is exactly "v" + the Cargo.toml package version.
# Used by .github/workflows/release.yml before any artifact is built.
set -eu

tag=${1:?usage: check-release-version.sh vX.Y.Z}
version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' Cargo.toml | head -n 1)

if [ -z "$version" ]; then
	echo "cannot read the package version from Cargo.toml" >&2
	exit 1
fi

if [ "v$version" != "$tag" ]; then
	echo "release tag $tag does not match Cargo.toml version v$version" >&2
	exit 1
fi

echo "release tag $tag matches Cargo.toml version $version"
