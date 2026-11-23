#!/bin/bash
# shellcheck disable=SC1091,SC2034  # sources the vars and publish_layers.sh

set -euo pipefail

. "${BASH_SOURCE%/*}/set_govcloud_vars.sh"
REGIONS=("${REGULAR_REGIONS[@]}")
. "${BASH_SOURCE%/*}/publish_layers.sh"
