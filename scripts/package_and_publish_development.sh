#!/bin/bash

set -euo pipefail

export AWS_PROFILE=${1:-presearch}
REGION=${2:-eu-west-1}
REGIONS=("$REGION")
NAME_SUFFIX="-development"
PUBLIC=false

. ${BASH_SOURCE%/*}/package_mac_x86_64.sh
. ${BASH_SOURCE%/*}/package_mac_arm64.sh
. ${BASH_SOURCE%/*}/publish_layers.sh