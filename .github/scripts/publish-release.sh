#!/usr/bin/env bash

set -euo pipefail

usage() {
  echo "usage: $0 version REPOSITORY VERSION SHA NOTES_FILE ASSET_DIR" >&2
  echo "       $0 tip REPOSITORY SHA ASSET_DIR" >&2
  exit 2
}

[[ $# -ge 1 ]] || usage

mode=$1
shift

expected_assets() {
  local version=$1
  local arch

  for arch in aarch64 armv7 i686 x86_64; do
    printf 'tuigreety-%s-%s.tar.gz\n' "$version" "$arch"
    printf 'tuigreety-%s-%s.tar.gz.sha256\n' "$version" "$arch"
  done
}

asset_json() {
  local repository=$1
  local release=$2

  gh release view "$release" --repo "$repository" --json assets
}

asset_field() {
  local json=$1
  local name=$2
  local field=$3

  jq --raw-output --arg name "$name" --arg field "$field" \
    '.assets[] | select(.name == $name) | .[$field]' <<< "$json"
}

validate_local_assets() {
  local version=$1
  local directory=$2
  local name
  local -a expected=()

  mapfile -t expected < <(expected_assets "$version")

  if [[ ! -d $directory ]]; then
    echo "asset directory not found: $directory" >&2
    exit 1
  fi

  if [[ $(find "$directory" -mindepth 1 -maxdepth 1 -type f | wc -l) -ne ${#expected[@]} ]] ||
    [[ -n $(find "$directory" -mindepth 1 -maxdepth 1 ! -type f -print -quit) ]]; then
    echo "asset directory must contain exactly ${#expected[@]} regular files" >&2
    exit 1
  fi

  for name in "${expected[@]}"; do
    if [[ ! -s $directory/$name ]] || [[ -L $directory/$name ]]; then
      echo "missing or unsafe release asset: $name" >&2
      exit 1
    fi
  done

  for name in "$directory"/*.sha256; do
    (cd "$directory" && sha256sum --check "${name##*/}")
  done
}

verify_remote_assets() {
  local repository=$1
  local release=$2
  local version=$3
  local directory=$4
  local json name digest local_digest state
  local -a expected=()

  mapfile -t expected < <(expected_assets "$version")
  json=$(asset_json "$repository" "$release")

  if [[ $(jq '.assets | length' <<< "$json") -ne ${#expected[@]} ]]; then
    echo "release $release does not contain exactly ${#expected[@]} assets" >&2
    return 1
  fi

  for name in "${expected[@]}"; do
    digest=$(asset_field "$json" "$name" digest)
    state=$(asset_field "$json" "$name" state)
    local_digest="sha256:$(sha256sum "$directory/$name" | cut -d ' ' -f 1)"
    if [[ $state != uploaded ]] || [[ $digest != "$local_digest" ]]; then
      echo "release asset $name is missing or has the wrong digest" >&2
      return 1
    fi
  done
}

latest_flag() {
  local repository=$1
  local version=$2
  local latest

  latest=$(
    gh api --paginate "repos/$repository/releases?per_page=100" \
      --jq '.[] | select(.draft == false and .prerelease == false) | .tag_name' |
      awk '/^[0-9]+\.[0-9]+\.[0-9]+$/' |
      sort --version-sort |
      tail -n 1
  )

  if [[ -z $latest ]] || [[ $(printf '%s\n' "$latest" "$version" | sort --version-sort | tail -n 1) == "$version" ]]; then
    printf '%s\n' --latest
  else
    printf '%s\n' --latest=false
  fi
}

publish_version() {
  [[ $# -eq 5 ]] || usage

  local repository=$1
  local version=$2
  local sha=$3
  local notes_file=$4
  local directory=$5
  local release_json draft assets name digest local_digest release_latest
  local -a expected=()

  validate_local_assets "$version" "$directory"
  mapfile -t expected < <(expected_assets "$version")
  release_latest=$(latest_flag "$repository" "$version")

  if gh release view "$version" --repo "$repository" >/dev/null 2>&1; then
    release_json=$(gh release view "$version" --repo "$repository" --json tagName,isDraft,isPrerelease)
    if [[ $(jq --raw-output '.tagName' <<< "$release_json") != "$version" ]] ||
      [[ $(jq --raw-output '.isPrerelease' <<< "$release_json") != false ]]; then
      echo "existing release $version has unexpected metadata" >&2
      exit 1
    fi
    draft=$(jq --raw-output '.isDraft' <<< "$release_json")
  else
    gh release create "$version" --repo "$repository" --verify-tag --target "$sha" \
      --draft --title "$version" --notes-file "$notes_file"
    draft=true
  fi

  if [[ $draft == true ]]; then
    for name in "${expected[@]}"; do
      assets=$(asset_json "$repository" "$version")
      digest=$(asset_field "$assets" "$name" digest)
      local_digest="sha256:$(sha256sum "$directory/$name" | cut -d ' ' -f 1)"

      if [[ -n $digest ]] && [[ $digest != "$local_digest" ]]; then
        gh release delete-asset "$version" "$name" --repo "$repository" --yes
        digest=
      fi

      if [[ -z $digest ]]; then
        gh release upload "$version" "$directory/$name" --repo "$repository"
      fi
    done

    assets=$(asset_json "$repository" "$version")
    while IFS= read -r name; do
      if ! printf '%s\n' "${expected[@]}" | grep --fixed-strings --line-regexp --quiet "$name"; then
        gh release delete-asset "$version" "$name" --repo "$repository" --yes
      fi
    done < <(jq --raw-output '.assets[].name' <<< "$assets")

    verify_remote_assets "$repository" "$version" "$version" "$directory"
    gh release edit "$version" --repo "$repository" --verify-tag --title "$version" \
      --notes-file "$notes_file" --draft=false --prerelease=false "$release_latest"
  else
    verify_remote_assets "$repository" "$version" "$version" "$directory"
    gh release edit "$version" --repo "$repository" --verify-tag --title "$version" \
      --notes-file "$notes_file" --prerelease=false "$release_latest"
  fi

  local expected_body remote_body
  expected_body=$(< "$notes_file")
  remote_body=$(gh release view "$version" --repo "$repository" --json body --jq '.body')
  if [[ $remote_body != "$expected_body" ]]; then
    echo "published release notes do not match $notes_file" >&2
    exit 1
  fi
}

publish_tip() {
  [[ $# -eq 3 ]] || usage

  local repository=$1
  local sha=$2
  local directory=$3
  local version=tip
  local current_sha assets name stage_name digest local_digest api_url asset_id
  local staging_directory
  local -a expected=()

  validate_local_assets "$version" "$directory"
  mapfile -t expected < <(expected_assets "$version")

  current_sha=$(gh api "repos/$repository/commits/master" --jq '.sha')
  if [[ $current_sha != "$sha" ]]; then
    echo "not publishing obsolete tip build $sha; master is $current_sha"
    return
  fi

  if ! gh release view tip --repo "$repository" >/dev/null 2>&1; then
    gh release create tip "$directory"/* --repo "$repository" --target "$sha" \
      --prerelease --title tip --notes 'Continuous build from master.'
    verify_remote_assets "$repository" tip tip "$directory"
    return
  fi

  staging_directory=$(mktemp -d)

  for name in "${expected[@]}"; do
    stage_name="$name.staged-$sha"
    install -m644 "$directory/$name" "$staging_directory/$stage_name"
    assets=$(asset_json "$repository" tip)
    digest=$(asset_field "$assets" "$stage_name" digest)
    local_digest="sha256:$(sha256sum "$directory/$name" | cut -d ' ' -f 1)"

    if [[ -n $digest ]] && [[ $digest != "$local_digest" ]]; then
      gh release delete-asset tip "$stage_name" --repo "$repository" --yes
      digest=
    fi

    if [[ -z $digest ]]; then
      gh release upload tip "$staging_directory/$stage_name" --repo "$repository"
    fi
  done

  current_sha=$(gh api "repos/$repository/commits/master" --jq '.sha')
  if [[ $current_sha != "$sha" ]]; then
    echo "master advanced to $current_sha while staging tip assets; leaving the current tip unchanged"
    return
  fi

  for name in "${expected[@]}"; do
    stage_name="$name.staged-$sha"
    assets=$(asset_json "$repository" tip)
    digest=$(asset_field "$assets" "$name" digest)
    local_digest="sha256:$(sha256sum "$directory/$name" | cut -d ' ' -f 1)"

    if [[ $digest == "$local_digest" ]]; then
      gh release delete-asset tip "$stage_name" --repo "$repository" --yes
      continue
    fi

    if [[ -n $digest ]]; then
      gh release delete-asset tip "$name" --repo "$repository" --yes
    fi

    assets=$(asset_json "$repository" tip)
    api_url=$(asset_field "$assets" "$stage_name" apiUrl)
    if [[ -z $api_url ]]; then
      echo "staged asset disappeared: $stage_name" >&2
      exit 1
    fi
    asset_id=${api_url##*/}
    gh api --method PATCH "repos/$repository/releases/assets/$asset_id" --field name="$name" >/dev/null
  done

  assets=$(asset_json "$repository" tip)
  while IFS= read -r name; do
    if ! printf '%s\n' "${expected[@]}" | grep --fixed-strings --line-regexp --quiet "$name"; then
      gh release delete-asset tip "$name" --repo "$repository" --yes
    fi
  done < <(jq --raw-output '.assets[].name' <<< "$assets")

  verify_remote_assets "$repository" tip tip "$directory"
  gh api --method PATCH "repos/$repository/git/refs/tags/tip" --field sha="$sha" --field force=true >/dev/null
  gh release edit tip --repo "$repository" --target "$sha" --title tip --prerelease \
    --notes 'Continuous build from master.'
}

case "$mode" in
  version)
    publish_version "$@"
    ;;
  tip)
    publish_tip "$@"
    ;;
  *)
    usage
    ;;
esac
