#!/bin/sh
# Host unit tests of the HY3131 model. Needs a .NET 8 SDK (DOTNET=path/to/dotnet
# or on PATH; else the fallback below, a local SDK under ~).
set -e
cd "$(dirname "$0")"
DOTNET=${DOTNET:-$(command -v dotnet || echo "$HOME/work/board/tmp/topor/_tools/dotnet/dotnet")}
export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1
exec "$DOTNET" run --nologo -v q -- "$@"
