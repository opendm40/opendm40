#!/bin/sh
# Render the diagrams in this directory to SVG.
#   overview.mmd, mcu.mmd -> Mermaid (mermaid-cli: npm install @mermaid-js/mermaid-cli;
#                            set MMDC to its path if it is not on PATH)
#   dmm_frontend.py       -> schemdraw (pip install schemdraw; set PYTHON if needed)
# Extra arguments go to mmdc, e.g. ./render.sh -p puppeteer.json
# with puppeteer.json = {"args":["--no-sandbox"]} where Chrome needs it.
set -e
cd "$(dirname "$0")"
MMDC=${MMDC:-mmdc}
PYTHON=${PYTHON:-python3}
for f in overview mcu; do
  "$MMDC" -q -i "$f.mmd" -o "$f.svg" -c mermaid.json -b white "$@"
done
"$PYTHON" dmm_frontend.py
