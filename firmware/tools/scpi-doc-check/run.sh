#!/bin/bash
# Check that docs/manual/scpi-reference.md and the SCPI parser in
# firmware/src name the same command mnemonics (check.py: the rules, the
# row format). Needs only python3. Exit 1 on any difference.
#   tools/scpi-doc-check/run.sh [--src DIR] [--doc FILE] [-v]
set -e
exec python3 "$(dirname "$0")/check.py" "$@"
