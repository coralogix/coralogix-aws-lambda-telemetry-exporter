#!/bin/bash
set -euo pipefail
exec "$(dirname "${BASH_SOURCE[0]}")/zigcc-shim.sh" x86_64-linux-gnu.2.26 "$@"
