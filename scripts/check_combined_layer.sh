#!/bin/bash

# Prerequisites:
#  - Installed and configured AWS CLI
# Usage:
#  ENVIRONMENT=test RUNTIME=python ./check_combined_layer.sh

set -euo pipefail

if [ -z "${ENVIRONMENT:-}" ]; then
    echo "ENVIRONMENT must be set (test or production)."
    exit 1
fi

if [ -z "${RUNTIME:-}" ]; then
    echo "RUNTIME must be set (python, nodejs, or java)."
    exit 1
fi

# shellcheck source=/dev/null
source "${BASH_SOURCE%/*}/set_${ENVIRONMENT}_vars.sh"

REGIONS=("${REGULAR_REGIONS[@]}" ${LIMITED_REGIONS[@]+"${LIMITED_REGIONS[@]}"})

echo "Checking combined layers for RUNTIME=$RUNTIME (NAME_SUFFIX=${NAME_SUFFIX:-}) in ${#REGIONS[@]} regions..."

x86_version=$("${BASH_SOURCE%/*}/check_layer_version.sh" \
    "coralogix-${RUNTIME}-wrapper-and-exporter-x86_64${NAME_SUFFIX}" \
    "${REGIONS[@]}")

arm64_version=$("${BASH_SOURCE%/*}/check_layer_version.sh" \
    "coralogix-${RUNTIME}-wrapper-and-exporter-arm64${NAME_SUFFIX}" \
    "${REGIONS[@]}")

if [ "$x86_version" != "$arm64_version" ]; then
    echo "x86 latest version ($x86_version) does not match arm64 latest version ($arm64_version) for $RUNTIME."
    exit 1
fi
