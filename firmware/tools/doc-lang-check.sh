#!/bin/sh
# The English, Russian and Chinese READMEs and user manuals stay in step (CI,
# the doc checks in .github/workflows/ci.yml; AGENTS.md "Docs in three
# languages"):
#  1. each translation has the structure of the English file
#     (tools/doc-lang-struct.py: headings, tables, list items, images, links,
#     code spans and ``` blocks; in Chinese also emphasis that would not render);
#  2. with BASE: a change since the merge base of BASE and HEAD that touches
#     one file of a group touches all of them. Escape hatch for a typo fix
#     in one language: a line "Doc-lang: one-language" in a commit message
#     of the range or in $PR_BODY (CI passes the PR description).
#   tools/doc-lang-check.sh [BASE [HEAD]]   (HEAD default: HEAD)
set -e
cd "$(dirname "$0")/../.."
BASE=$1; HEAD=${2:-HEAD}
# English file; the translations are <name>.ru.md and <name>.zh.md.
DOCS="README docs/manual/README"
LANGS="ru zh"
bad=0
for d in $DOCS; do
    en=$d.md
    [ -f "$en" ] || { echo "doc-lang-check: $en is missing" >&2; bad=1; continue; }
    for l in $LANGS; do
        tr=$d.$l.md
        [ -f "$tr" ] || { echo "doc-lang-check: $tr is missing" >&2; bad=1; continue; }
        opt=; [ "$l" = zh ] && opt=--markup
        if ! python3 firmware/tools/doc-lang-struct.py $opt "$en" "$tr" 2> "${TMPDIR:-/tmp}/dlc.$$"; then
            echo "doc-lang-check: $en and $tr differ (English / translated):" >&2
            cat "${TMPDIR:-/tmp}/dlc.$$" >&2
            bad=1
        fi
        rm -f "${TMPDIR:-/tmp}/dlc.$$"
    done
done
if [ -n "$BASE" ]; then
    MB=$(git merge-base "$BASE" "$HEAD")
    changed=$(git diff --name-only "$MB" "$HEAD")
    escape=no
    if git log --format=%B "$MB..$HEAD" | grep -q '^Doc-lang: one-language' ||
       printf '%s\n' "$PR_BODY" | tr -d '\r' | grep -q '^Doc-lang: one-language'; then
        escape=yes
    fi
    for d in $DOCS; do
        did=; missed=
        for f in "$d.md" "$d.ru.md" "$d.zh.md"; do
            if echo "$changed" | grep -qxF "$f"; then did="$did $f"; else missed="$missed $f"; fi
        done
        [ -z "$did" ] || [ -z "$missed" ] && continue
        if [ $escape = yes ]; then
            echo "doc-lang-check: only$did changed; allowed by \"Doc-lang: one-language\""
        else
            echo "doc-lang-check:$did changed since $(echo "$MB" | cut -c1-7) but$missed did not." >&2
            echo "  Make the same change in all languages, or, for a typo fix in one language only," >&2
            echo "  add the line \"Doc-lang: one-language\" to a commit message or the PR description." >&2
            bad=1
        fi
    done
fi
[ $bad = 0 ] && echo "doc-lang-check: ok"
exit $bad
