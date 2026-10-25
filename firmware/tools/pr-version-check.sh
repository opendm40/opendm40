#!/bin/sh
# Pull request guard (CI, .github/workflows/ci.yml): a PR that changes the
# firmware image's inputs must raise the version in Cargo.toml above the
# base branch's, by SemVer (firmware/README.md "Releasing").
#   tools/pr-version-check.sh BASE_SHA [HEAD_SHA]   (HEAD default: HEAD)
set -e
cd "$(dirname "$0")/.."
BASE=$1; HEAD=${2:-HEAD}
[ -n "$BASE" ] || { echo "pr-version-check: no base commit" >&2; exit 1; }
# What goes into the image (the same list as tools/version-check.sh).
INPUTS="src Cargo.toml Cargo.lock link-native.x build.rs .cargo vendor"
ver() { git show "$1:firmware/Cargo.toml" | sed -n 's/^version = "\(.*\)"/\1/p' | head -1; }
B=$(ver "$BASE"); H=$(ver "$HEAD")
if git diff --quiet "$BASE" "$HEAD" -- $INPUTS; then
    echo "pr-version-check: ok, the firmware inputs are unchanged ($H)"
    exit 0
fi
# sort -V orders 0.2.1 after 0.2.0; it puts 0.3.0-rc1 after 0.3.0, unlike
# SemVer, so a pre-release must not follow its own release.
if [ "$(printf '%s\n%s\n' "$B" "$H" | sort -V | tail -1)" = "$H" ] && [ "$H" != "$B" ]; then
    echo "pr-version-check: ok, $B -> $H"
    exit 0
fi
echo "pr-version-check: the firmware changes but the version stays $H (base $B):" >&2
git diff --stat "$BASE" "$HEAD" -- $INPUTS | tail -1 >&2
echo "raise version in firmware/Cargo.toml above $B (SemVer: PATCH for fixes, MINOR for features), then ./build.sh to update Cargo.lock" >&2
exit 1
