#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 VERSION EXPECTED_SHA NOTES_OUTPUT" >&2
  exit 2
fi

version=$1
expected_sha=$2
notes_output=$3
root=$(git rev-parse --show-toplevel)

cd "$root"

if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "invalid release version: $version" >&2
  exit 1
fi

head_sha=$(git rev-parse HEAD)
tag_sha=$(git rev-list -n 1 "refs/tags/$version")

if [[ $head_sha != "$expected_sha" ]]; then
  echo "checkout $head_sha does not match expected commit $expected_sha" >&2
  exit 1
fi

if [[ $tag_sha != "$expected_sha" ]]; then
  echo "tag $version points to $tag_sha, not $expected_sha" >&2
  exit 1
fi

cargo_version=$(cargo metadata --locked --no-deps --format-version 1 | jq --raw-output '.packages[0].version')
if [[ $cargo_version != "$version" ]]; then
  echo "Cargo package version $cargo_version does not match tag $version" >&2
  exit 1
fi

for package in greetd-tuigreety greetd-tuigreety-bin; do
  pkgbuild="packaging/aur/$package/PKGBUILD"
  package_version=$(sed -n 's/^pkgver=//p' "$pkgbuild")
  if [[ $package_version != "$version" ]]; then
    echo "$pkgbuild has pkgver=$package_version, expected $version" >&2
    exit 1
  fi
done

git_pkgbuild=packaging/aur/greetd-tuigreety-git/PKGBUILD
git_package_version=$(sed -n 's/^pkgver=//p' "$git_pkgbuild")
if [[ ! $git_package_version =~ ^[0-9]+\.[0-9]+\.[0-9]+\.r[0-9]+\.g[0-9a-f]{7,}$ ||
      $git_package_version =~ \.g0+$ ]]; then
  echo "$git_pkgbuild has non-concrete pkgver=$git_package_version" >&2
  exit 1
fi

mkdir -p "$(dirname "$notes_output")"
bash .github/scripts/release-notes.sh "$version" CHANGELOG.md > "$notes_output"

if [[ ! -s $notes_output ]]; then
  echo "release notes are empty" >&2
  exit 1
fi
