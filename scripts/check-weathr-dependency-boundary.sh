#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

tree="$(cargo tree -p weathr)"
for forbidden in ascii-assets watchdog toml serde_json; do
  if printf '%s\n' "$tree" | grep -Eq "(^|[^[:alnum:]-])${forbidden}[[:space:]]v"; then
    echo "weathr must not depend on ${forbidden}" >&2
    printf '%s\n' "$tree" >&2
    exit 1
  fi
done

if ! printf '%s\n' "$tree" | grep -Eq '(^|[^[:alnum:]-])system-services-model[[:space:]]v'; then
  echo "weathr must retain its system-services-model DTO boundary" >&2
  printf '%s\n' "$tree" >&2
  exit 1
fi
