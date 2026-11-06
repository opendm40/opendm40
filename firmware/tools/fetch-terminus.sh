#!/bin/sh
# The Terminus Font 4.49.1 BDF files tools/mkterm.py draws DM40 Term from
# (not in the repo; SIL OFL 1.1, fonts/OFL-DM40Term.txt), into
# target/terminus-font-4.49.1 (or the directory given), with Terminus's own
# alternative Cyrillic letters alt/ij1 (и й with the diagonal) and alt/dv1
# (д в in the Russian shape; the default ones are the Bulgarian). Checks the
# tarball's SHA-256; mkterm.py checks each BDF's.
#   tools/fetch-terminus.sh [dir]
set -e
cd "$(dirname "$0")/.."
D=${1:-target}
URL=https://downloads.sourceforge.net/project/terminus-font/terminus-font-4.49/terminus-font-4.49.1.tar.gz
SHA=d961c1b781627bf417f9b340693d64fc219e0113ad3a3af1a3424c7aa373ef79
mkdir -p "$D"
T="$D/terminus-font-4.49.1.tar.gz"
[ -f "$T" ] || curl -fsSL -o "$T" "$URL"
echo "$SHA  $T" | sha256sum -c -
rm -rf "$D/terminus-font-4.49.1"
tar -xzf "$T" -C "$D"
(cd "$D/terminus-font-4.49.1" && patch -s -p1 -i alt/ij1.diff && patch -s -p1 -i alt/dv1.diff)
echo "BDF files in $D/terminus-font-4.49.1"
