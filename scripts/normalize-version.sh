#!/usr/bin/env bash
#
# Normalize a raw version string (tag or dispatch input) to a bare X.Y.Z.
#
# Both `git push tag v0.2.0` and a manual `workflow_dispatch` input of "v0.2.0"
# (or "0.2.0") must collapse to the same value. Stripping ONE leading 'v'
# avoids pushing a "vv0.2.0" tag and emitting a manifest whose version fails
# semver parsing client-side.
#
# Usage:
#   scripts/normalize-version.sh <raw>          # prints "0.2.0"
#   VALUE="$(scripts/normalize-version.sh v0.2.0)" && [ -z "$VALUE" ] && exit 1
#
# Exits non-zero (with a message on stderr) if the result is not X.Y.Z, so this
# can be used inline: VALUE="$(scripts/normalize-version.sh "$RAW")" || exit 1.
set -euo pipefail

if [ "$#" -lt 1 ] || [ -z "${1:-}" ]; then
  echo "Usage: $0 <raw-version>" >&2
  exit 1
fi

RAW="$1"
# Strip ONE leading 'v' so a dispatch typo ("v0.2.0") and the tag form
# ("v0.2.0") both normalize to "0.2.0".
VALUE="${RAW#v}"

if ! printf '%s' "$VALUE" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "Error: invalid version '$VALUE' (expected X.Y.Z)" >&2
  exit 1
fi

printf '%s' "$VALUE"
