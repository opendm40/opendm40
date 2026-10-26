#!/bin/sh
# Release notes for a tag (.github/workflows/release.yml): a fixed header
# (install, backup, recovery), then the tag's section of ../CHANGELOG.md
# ("## v0.1-pre2 ..." up to the next "## "), or, when CHANGELOG.md has no
# such section, the text of AUTO (GitHub's generated notes: the merged PRs
# and the commits since the previous tag).
#   tools/release-notes.sh TAG COMMIT [AUTO]   > notes.md
set -e
TAG=$1; COMMIT=$2; AUTO=$3
[ -n "$TAG" ] && [ -n "$COMMIT" ] || { echo "usage: release-notes.sh TAG COMMIT [AUTO]" >&2; exit 2; }
[ -z "$AUTO" ] || AUTO=$(cd "$(dirname "$AUTO")" && pwd)/$(basename "$AUTO")
cd "$(dirname "$0")/.."
REPO=${GITHUB_REPOSITORY:-wirenboard/opendm40}
BLOB=https://github.com/$REPO/blob/$TAG
NAME=dm40_opendm40_$TAG.bin
cat <<EOF
opendm40 $TAG, the replacement firmware for the Alientek DM40 (A / B / C), built from commit $(echo "$COMMIT" | cut -c1-7) by CI.

**Back up first:** after installing, copy \`INFO/\` and \`DEBUG/SPIFLASH.BIN\` from the meter's USB disk to your PC; the calibration and boot data cannot be rebuilt ([Back up your meter]($BLOB/docs/manual/README.md#back-up-your-meter)).

**Install:** copy \`$NAME\` onto the meter's USB disk as the only \`dm40*.bin\` and power the meter off and on; the first start takes a few seconds ([Install]($BLOB/README.md#install)).

**Recovery and back to stock:** a bad image cannot brick the meter: the vendor loader's update screen takes a good file ([Backup, recovery and back to stock]($BLOB/docs/manual/README.md#backup-recovery-and-back-to-stock)); the official [\`dm40_v_2_6.bin\`]($BLOB/stock/vendor/dm40_v_2_6.bin) restores the stock firmware.

Manual: [docs/manual/README.md]($BLOB/docs/manual/README.md). \`SHA256SUMS\`: sha256 of the image.

EOF
SECTION=$(awk -v t="$TAG" '
    /^## / { if (on) exit; on = ($2 == t) ; next }
    on' ../CHANGELOG.md 2>/dev/null)
if [ -n "$(echo "$SECTION" | tr -d '[:space:]')" ]; then
    echo "## Changes"
    # Relative links (repo root) -> links at the tag.
    echo "$SECTION" | sed -E "s#\]\(([^):\#][^):]*)\)#](${BLOB}/\1)#g"
    PREV=$(git describe --tags --match 'v*' --abbrev=0 "$COMMIT^" 2>/dev/null) &&
        printf '\n**Full changelog:** https://github.com/%s/compare/%s...%s\n' "$REPO" "$PREV" "$TAG" || true
elif [ -n "$AUTO" ] && [ -s "$AUTO" ]; then
    cat "$AUTO"
fi
