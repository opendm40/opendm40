#!/bin/bash
# The manual's pictures of one language: docs/manual/img/<lang>/ (see manual-img.py).
#   tools/ui-render/manual-lang.sh ru|zh [work dir]
# The screens are drawn by that language's firmware text (UIR_LANG); every run
# keeps ui-render's layout rules, and manual-annot.py its text checks.
set -e
L=$1; [ "$L" = ru ] || [ "$L" = zh ] || { echo "usage: manual-lang.sh ru|zh [work dir]" >&2; exit 2; }
cd "$(dirname "$0")"
W=${2:-$(mktemp -d)}
mkdir -p "$W/man" "$W/readme" "$W/matrix" "$W/stock"
UIR_LANG=$L ./run.sh "$W/man" --manual | tail -2
UIR_LANG=$L ./run.sh "$W/readme" --readme | tail -2
UIR_LANG=$L UIR_SAVE_ONLY=m04-main-menu,i02-inrush-dc ./run.sh "$W/matrix" | tail -2
../stock-ui/run.sh "$W/stock" > /dev/null
MANUAL_LANG=$L /usr/bin/python3 -I manual-img.py "$W/man" "$W/readme" "$W/matrix" "$W/none" "$W/stock"
