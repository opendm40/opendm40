#!/bin/sh
# The UI languages (src/lang.rs, i18n/): every language has the same strings,
# every UI string is translated or English on purpose (i18n/english.txt), and
# the generated tables match the tsv files. CI: tools/ci-tests.sh.
cd "$(dirname "$0")/../.." || exit 1
python3 -I tools/lang-check/check.py || exit 1
for l in zh ru it fr; do
    (cd tools && python3 -I mklang.py "$l" --check) || exit 1
done
