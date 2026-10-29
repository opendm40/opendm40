#!/bin/bash
# Post (or update) the "test build" comment on a pull request after CI
# (.github/workflows/pr-build.yml, firmware/README.md "PR builds").
#   tools/pr-build-comment.sh RUN_ID [--dry-run]
# RUN_ID: a CI run of a pull_request event. Needs gh with a token that may
# read the run's artifacts and write PR comments (GH_TOKEN, GH_REPO).
# The CI run built untrusted PR code: its artifact is only data here. The
# script never runs anything from it; it takes the PR number from the
# artifact name, checks it against the run's head commit, unzips into an
# empty directory and hashes the .bin. A run that is not the PR's current
# head (an older push) posts nothing.
set -euo pipefail
RUN=${1:?usage: pr-build-comment.sh RUN_ID [--dry-run]}
DRY=${2:-}
[[ $RUN =~ ^[0-9]+$ ]] || { echo "pr-build-comment: bad run id $RUN" >&2; exit 1; }
REPO=${GH_REPO:?GH_REPO (owner/repo)}
SERVER=${GITHUB_SERVER_URL:-https://github.com}
# The comment's author: the workflow's token posts as github-actions[bot].
AUTHOR=${COMMENT_AUTHOR:-github-actions[bot]}
MARK='<!-- opendm40-pr-build -->'

read -r EVENT HEAD CONCL RUNNUM < <(gh api "repos/$REPO/actions/runs/$RUN" \
    --jq '[.event, .head_sha, (.conclusion // "running"), .run_number] | @tsv')
[ "$EVENT" = pull_request ] || { echo "pr-build-comment: run $RUN is a $EVENT run, nothing to do"; exit 0; }

# The image artifact: dm40_opendm40_v<version>-pr<N>.<head7> (ci.yml).
ART=$(gh api "repos/$REPO/actions/runs/$RUN/artifacts" \
    --jq '.artifacts[] | select(.expired | not) | select(.name | test("^dm40_opendm40_v[0-9A-Za-z.-]+-pr[0-9]+\\.[0-9a-f]{7}$")) | [.id, .name, .expires_at] | @tsv' | head -1)
[ -n "$ART" ] || { echo "pr-build-comment: run $RUN has no image artifact (docs-only change or a failed build)"; exit 0; }
read -r ART_ID NAME EXPIRES <<< "$ART"
[[ $NAME =~ -pr([0-9]+)\.([0-9a-f]{7})$ ]]
PR=${BASH_REMATCH[1]}; SHORT=${BASH_REMATCH[2]}
VER=${NAME#dm40_opendm40_v}

# The PR named in the artifact name must have this run's commit as its head.
PRHEAD=$(gh api "repos/$REPO/pulls/$PR" --jq .head.sha)
if [ "$PRHEAD" != "$HEAD" ] || [ "${HEAD:0:7}" != "$SHORT" ]; then
    echo "pr-build-comment: PR #$PR head is $PRHEAD, run $RUN built $HEAD ($SHORT): not the current head, no comment"
    exit 0
fi

DIR=$(mktemp -d)
trap 'rm -rf "$DIR"' EXIT
gh run download "$RUN" -R "$REPO" -n "$NAME" -D "$DIR/a"
BIN=$DIR/a/$NAME.bin
[ -f "$BIN" ] || { echo "pr-build-comment: $NAME.bin not in the artifact" >&2; exit 1; }
SUM=$(sha256sum "$BIN" | cut -d' ' -f1)
# BUILD.txt (ci.yml): the commit CI built, the PR head merged into the base.
MERGE=$(sed -n 's/^merge \([0-9a-f]\{40\}\)$/\1/p' "$DIR/a/BUILD.txt" 2>/dev/null || true)
BASE=$(sed -n 's/^base \([0-9a-f]\{40\}\)$/\1/p' "$DIR/a/BUILD.txt" 2>/dev/null || true)

C=$SERVER/$REPO/commit
built="PR head [\`${HEAD:0:7}\`]($C/$HEAD)"
[ -n "$BASE" ] && built="$built merged into \`main\` at [\`${BASE:0:7}\`]($C/$BASE)"
[ -n "$MERGE" ] && built="$built (merge commit \`${MERGE:0:7}\`)"
MAN=$SERVER/$REPO/blob/main/docs/manual/README.md
BODY=$(cat <<EOF
$MARK
### Test build of this pull request

\`$NAME.bin\`: the meter shows the version \`$VER\` (start screen, Settings > More > More > Device, \`*IDN?\`).

| | |
|---|---|
| Download, no GitHub account needed | [$NAME.zip](https://nightly.link/$REPO/actions/runs/$RUN/$NAME.zip) (via [nightly.link](https://nightly.link)) |
| Download, GitHub account | [artifact]($SERVER/$REPO/actions/runs/$RUN/artifacts/$ART_ID) of [CI run #$RUNNUM]($SERVER/$REPO/actions/runs/$RUN) (CI result: $CONCL) |
| SHA-256 of the .bin | \`$SUM\` |
| Built from | $built |
| Available until | ${EXPIRES%%T*} |

> [!WARNING]
> A test build of an unreviewed, unmerged change: not a release, not tested on a meter. Install it like an update ([manual, Update]($MAN#update)). To go back, install the [latest release]($SERVER/$REPO/releases/latest); if the meter does not start, see [Backup, recovery and back to stock]($MAN#backup-recovery-and-back-to-stock).

Updated for every push to this pull request.
EOF
)
if [ "$DRY" = --dry-run ]; then
    echo "$BODY"
    exit 0
fi
ID=$(gh api --paginate "repos/$REPO/issues/$PR/comments" \
    --jq ".[] | select(.user.login == \"$AUTHOR\") | select(.body | startswith(\"$MARK\")) | .id" | head -1)
if [ -n "$ID" ]; then
    gh api -X PATCH "repos/$REPO/issues/comments/$ID" -f body="$BODY" --jq .html_url
else
    gh api -X POST "repos/$REPO/issues/$PR/comments" -f body="$BODY" --jq .html_url
fi
