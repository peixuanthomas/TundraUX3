#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Select Weathr alone: workspace builds deliberately enable the runtime through Shell.
tree="$(cargo tree --locked -p weathr)"
for forbidden in ascii-assets watchdog platform reqwest chrono-tz time toml serde_json; do
  if printf '%s\n' "$tree" | grep -Eq "(^|[^[:alnum:]-])${forbidden}[[:space:]]v"; then
    echo "weathr must not depend on ${forbidden}" >&2
    printf '%s\n' "$tree" >&2
    exit 1
  fi
done

if ! printf '%s\n' "$tree" | grep -Eq '(^|[^[:alnum:]-])system-services[[:space:]]v'; then
  echo "weathr must consume shared snapshots from system-services" >&2
  printf '%s\n' "$tree" >&2
  exit 1
fi
