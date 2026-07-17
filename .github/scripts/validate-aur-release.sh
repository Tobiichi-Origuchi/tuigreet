#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 VERSION REPOSITORY" >&2
  exit 2
fi

version=$1
repository=$2
root=$(git rev-parse --show-toplevel)
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT

cd "$root"

head_sha=$(git rev-parse HEAD)
bash .github/scripts/validate-release.sh "$version" "$head_sha" "$temporary/release-notes.md"

release_json=$(gh release view "$version" --repo "$repository" \
  --json tagName,isDraft,isPrerelease,body,assets)

if [[ $(jq --raw-output '.tagName' <<< "$release_json") != "$version" ]] ||
  [[ $(jq --raw-output '.isDraft' <<< "$release_json") != false ]] ||
  [[ $(jq --raw-output '.isPrerelease' <<< "$release_json") != false ]]; then
  echo "GitHub release $version is missing, draft, or a prerelease" >&2
  exit 1
fi

expected_body=$(< "$temporary/release-notes.md")
release_body=$(jq --raw-output '.body' <<< "$release_json")
if [[ $release_body != "$expected_body" ]]; then
  echo "GitHub release notes do not match CHANGELOG.md" >&2
  exit 1
fi

if [[ $(jq '.assets | length' <<< "$release_json") -ne 8 ]]; then
  echo "GitHub release must contain exactly four archives and four checksums" >&2
  exit 1
fi

gh release download "$version" --repo "$repository" --dir "$temporary" --pattern '*.sha256'

for arch in aarch64 armv7 i686 x86_64; do
  archive="tuigreety-$version-$arch.tar.gz"
  checksum="$archive.sha256"
  remote_digest=$(jq --raw-output --arg name "$archive" \
    '.assets[] | select(.name == $name and .state == "uploaded") | .digest' <<< "$release_json")
  remote_checksum_digest=$(jq --raw-output --arg name "$checksum" \
    '.assets[] | select(.name == $name and .state == "uploaded") | .digest' <<< "$release_json")
  read -r documented_digest documented_name < "$temporary/$checksum"

  if [[ $documented_name != "$archive" ]] || [[ $remote_digest != "sha256:$documented_digest" ]] ||
    [[ ! $remote_checksum_digest =~ ^sha256:[0-9a-f]{64}$ ]]; then
    echo "invalid or inconsistent release assets for $arch" >&2
    exit 1
  fi
done

aur_json=$(curl --fail --silent --show-error --get 'https://aur.archlinux.org/rpc/v5/info' \
  --data-urlencode 'arg[]=greetd-tuigreety' \
  --data-urlencode 'arg[]=greetd-tuigreety-bin')

for package in greetd-tuigreety greetd-tuigreety-bin; do
  pkgrel=$(sed -n 's/^pkgrel=//p' "packaging/aur/$package/PKGBUILD")
  candidate="$version-$pkgrel"
  current=$(jq --raw-output --arg package "$package" \
    '.results[] | select(.Name == $package) | .Version' <<< "$aur_json")

  if [[ -n $current ]] && (( $(vercmp "$candidate" "$current") < 0 )); then
    echo "refusing to downgrade $package from $current to $candidate" >&2
    exit 1
  fi
done
