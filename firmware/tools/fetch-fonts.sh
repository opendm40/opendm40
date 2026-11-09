#!/bin/sh
# The font sources the generators re-rasterise or load (not in the repo):
#   target/noto/NotoSans[wdth,wght].ttf   Noto Sans 2.015 (google/fonts, SIL OFL
#       1.1, fonts/OFL-NotoSans.txt): the text fonts, tools/mkttf.py
#   target/x11/6x13.bdf   X11 misc-fixed 6x13 from xorg font-misc-misc 1.1.3
#       (public domain, fonts/X11-misc-fixed.txt): F14, tools/mkterm.py
# Each download's SHA-256 is checked; mkterm.py checks the BDF's again.
#   tools/fetch-fonts.sh [dir]
set -e
cd "$(dirname "$0")/.."
D=${1:-target}
NOTO_URL='https://raw.githubusercontent.com/google/fonts/2984c575fdce412ee02b2baaba67672b9a9434d8/ofl/notosans/NotoSans%5Bwdth,wght%5D.ttf'
NOTO_SHA=bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d
MISC_URL=https://www.x.org/releases/individual/font/font-misc-misc-1.1.3.tar.xz
MISC_SHA=79abe361f58bb21ade9f565898e486300ce1cc621d5285bec26e14b6a8618fed
mkdir -p "$D/noto" "$D/x11"
N="$D/noto/NotoSans[wdth,wght].ttf"
[ -f "$N" ] || curl -fsSL -o "$N" "$NOTO_URL"
echo "$NOTO_SHA  $N" | sha256sum -c -
T="$D/x11/font-misc-misc-1.1.3.tar.xz"
[ -f "$T" ] || curl -fsSL -o "$T" "$MISC_URL"
echo "$MISC_SHA  $T" | sha256sum -c -
tar -xJf "$T" -C "$D/x11" --strip-components=1 font-misc-misc-1.1.3/6x13.bdf
echo "fonts in $D/noto and $D/x11"
