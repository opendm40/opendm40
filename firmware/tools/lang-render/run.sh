#!/bin/bash
# ui-render's layout rules on every localized image's screens (UIR_LANG), at
# the same time, and the build-time translation check (src/lang.rs): fails
# when a translation is wider than its box, lacks a glyph of its font or is
# drawn in a font under the language's MIN_LINE (lang.log kind `wide`).
# Kind `english`: a translated string drawn in English (no tr! there), listed.
#   tools/lang-render/run.sh [out dir] [lang ...]
cd "$(dirname "$0")/../.." || exit 1
OUT=${1:-$(mktemp -d)}; shift
LANGS=${*:-zh ru it fr}
for l in $LANGS; do
    mkdir -p "$OUT/$l"; : > "$OUT/$l/lang.log"
    (UIR_NOSAVE=1 UIR_LANG=$l LANG_LOG=$OUT/$l/lang.log tools/ui-render/run.sh "$OUT/$l" > "$OUT/$l/run.log" 2>&1; echo $? > "$OUT/$l/rc") &
done
wait
rc=0
for l in $LANGS; do
    echo "$l: $(grep -m1 '^cases' "$OUT/$l/run.log"), translations that do not fit: $(grep -c '^wide' "$OUT/$l/lang.log"), drawn in English: $(grep -c '^english' "$OUT/$l/lang.log")"
    [ "$(cat "$OUT/$l/rc")" = 0 ] || { rc=1; grep -E ' x  ' "$OUT/$l/run.log" | head -20; }
    grep '^wide' "$OUT/$l/lang.log" && rc=1
done
exit $rc
