#!/usr/bin/env bash
# Prints the newest exporter layer version that is public for both architectures in every
# GovCloud region. Only publish-govcloud.yml makes a version public, after its smoke test,
# so a newer private version left by a failed run is skipped, never merged.
#
#   ci/govcloud/public-exporter-version.sh
set -euo pipefail

# shellcheck source=/dev/null
. "${BASH_SOURCE%/*}/../../scripts/set_govcloud_vars.sh"

is_public() { # <layer> <version> <region>
  aws lambda get-layer-version-policy --region "$3" --layer-name "$1" --version-number "$2" \
    --query Policy --output text 2>/dev/null | jq -e '.Statement[] | select(.Effect == "Allow" and .Principal == "*" and .Condition == null)' >/dev/null
}

# Newest first.
versions=$(aws lambda list-layer-versions --region "${REGULAR_REGIONS[0]}" \
  --layer-name coralogix-aws-lambda-telemetry-exporter-x86_64 --query 'LayerVersions[].Version' --output text)
for version in $versions; do
  public=true
  for region in "${REGULAR_REGIONS[@]}"; do
    for arch in x86_64 arm64; do
      is_public "coralogix-aws-lambda-telemetry-exporter-$arch" "$version" "$region" || { public=false; break 2; }
    done
  done
  if $public; then echo "$version"; exit 0; fi
  echo "Exporter version $version is not public for both architectures in every region; skipping it." >&2
done
echo "::error::No exporter version is public for both architectures in every GovCloud region." >&2
exit 1
