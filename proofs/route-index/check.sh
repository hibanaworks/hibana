#!/bin/sh
set -eu
HERE=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
LEAN_BIN=${LEAN_BIN:-lean}
PYTHON=${PYTHON:-python3}
"$LEAN_BIN" --version | grep 'version 4.30.0'
"$LEAN_BIN" "$HERE/SortedRouteIndex.lean"
"$PYTHON" "$HERE/check_sorted_route_index.py"
