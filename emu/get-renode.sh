#!/bin/sh
# Fetch the portable Renode build (no sudo, self-contained .NET) into emu/.renode/.
# Usage: emu/get-renode.sh [version]   (default 1.17.0)
set -e
cd "$(dirname "$0")"
VER=${1:-1.17.0}
URL=https://github.com/renode/renode/releases/download/v$VER/renode-$VER.linux-portable.tar.gz
mkdir -p .renode
if [ ! -x ".renode/renode_$VER-portable/renode" ]; then
    echo "downloading $URL"
    curl -sSL -o ".renode/renode-$VER.tar.gz" "$URL"
    tar xzf ".renode/renode-$VER.tar.gz" -C .renode
    rm -f ".renode/renode-$VER.tar.gz"
fi
ln -sfn "renode_$VER-portable" .renode/current
".renode/current/renode" --version | head -1
