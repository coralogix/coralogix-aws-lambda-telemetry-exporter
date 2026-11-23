#!/bin/bash
# shellcheck disable=SC2034  # sourced by the publish and check scripts

set -euo pipefail

REGULAR_REGIONS=("us-gov-west-1" "us-gov-east-1")
LIMITED_REGIONS=()
NAME_SUFFIX=""
# Published private; publish-govcloud.yml makes the version public after the smoke test.
PUBLIC=false
# The GovCloud layer is built on AL2023 (ci/govcloud/README.md), so AL2023 runtimes only.
COMPATIBLE_RUNTIMES="nodejs22.x nodejs24.x nodejs26.x java21 python3.12 python3.13 python3.14 dotnet8 provided.al2023"
