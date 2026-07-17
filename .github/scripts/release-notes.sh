#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 VERSION CHANGELOG" >&2
  exit 2
fi

version=$1
changelog=$2

if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "invalid release version: $version" >&2
  exit 1
fi

if [[ ! -f $changelog ]]; then
  echo "changelog not found: $changelog" >&2
  exit 1
fi

awk -v prefix="## $version - " '
  BEGIN {
    found = 0
    emit = 0
    lines = 0
  }

  /^## / {
    if (index($0, prefix) == 1) {
      found++
      emit = 1
      next
    }

    emit = 0
  }

  emit {
    print
    lines++
  }

  END {
    if (found != 1 || lines == 0) {
      exit 1
    }
  }
' "$changelog" || {
  echo "CHANGELOG.md must contain exactly one nonempty '## $version - YYYY-MM-DD' section" >&2
  exit 1
}
