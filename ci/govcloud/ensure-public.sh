#!/usr/bin/env bash
# Makes a layer version public in every GovCloud region and fails unless it is. Safe to
# repeat: a version already made public (or whose earlier grant half-failed) is fine.
#
#   ci/govcloud/ensure-public.sh <layer-name> <version>
set -euo pipefail

usage="usage: ensure-public.sh <layer-name> <version>"
layer=${1:?$usage} version=${2:?$usage}

# shellcheck source=/dev/null
. "${BASH_SOURCE%/*}/../../scripts/set_govcloud_vars.sh"

for region in "${REGULAR_REGIONS[@]}"; do
  # Fails with ResourceConflictException when the permission is already there; the check
  # below decides.
  aws lambda add-layer-version-permission --region "$region" --layer-name "$layer" --version-number "$version" \
    --statement-id public --action lambda:GetLayerVersion --principal '*' >/dev/null 2>&1 || true
  aws lambda get-layer-version-policy --region "$region" --layer-name "$layer" --version-number "$version" \
    --query Policy --output text | jq -e '.Statement[] | select(.Effect == "Allow" and .Principal == "*" and .Condition == null)' >/dev/null \
    || { echo "::error::$layer v$version is not public in $region."; exit 1; }
  echo "$region $layer v$version: public"
done
