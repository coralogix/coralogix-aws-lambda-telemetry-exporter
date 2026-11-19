#!/usr/bin/env bash
# Publish one signed GovCloud layer ZIP to one region, privately, or reuse the version a
# previous run already published.
#
#   ci/govcloud/publish-layer.sh <arch> <region> <signed-zip> <description>
#
# A layer version is matched by its description ("v<version> <short-sha>"):
#   - one with the same CodeSha256 as this ZIP is reused, so a re-run publishes only
#     the regions that are missing;
#   - one with a different CodeSha256 fails: another build was published as this release.
# Version numbers are per region and are not compared across regions.
# Nothing here makes a version public.
#
# Prints one JSON line: {"arch","region","arn","code_sha256","reused"}.
set -euo pipefail

arch=$1 region=$2 zip=$3 description=$4
case "$arch" in x86_64|arm64) ;; *) echo "arch must be x86_64 or arm64, got '$arch'" >&2; exit 1 ;; esac
[ -f "$zip" ] || { echo "no such file: $zip" >&2; exit 1; }
[ -n "$description" ] || { echo "description is empty" >&2; exit 1; }

# AL2023 runtimes only (ci/govcloud/README.md). The AL2023 subset of the commercial list
# in scripts/publish_layers.sh, which the Lambda API already accepts.
COMPATIBLE_RUNTIMES=(nodejs22.x nodejs24.x python3.12 python3.13 python3.14 java21 provided.al2023)

layer=coralogix-aws-lambda-telemetry-exporter-$arch
expected=$(openssl dgst -sha256 -binary "$zip" | base64)

result() { # result <arn> <code_sha256> <reused>
  jq -nc --arg arch "$arch" --arg region "$region" --arg arn "$1" --arg sha "$2" --argjson reused "$3" \
    '{arch: $arch, region: $region, arn: $arn, code_sha256: $sha, reused: $reused}'
}

existing=$(aws lambda list-layer-versions --region "$region" --layer-name "$layer" --output json \
  | jq -r --arg d "$description" '.LayerVersions[] | select(.Description == $d) | .LayerVersionArn')

for arn in $existing; do
  sha=$(aws lambda get-layer-version-by-arn --region "$region" --arn "$arn" \
    --query Content.CodeSha256 --output text)
  if [ "$sha" != "$expected" ]; then
    echo "::error::$arn is already published as '$description' with CodeSha256 $sha, not this build's $expected." >&2
    exit 1
  fi
done
if [ -n "$existing" ]; then
  arn=$(head -1 <<<"$existing")
  echo "$region $arch: reusing $arn" >&2
  result "$arn" "$expected" true
  exit 0
fi

out=$(aws lambda publish-layer-version --region "$region" --layer-name "$layer" \
  --description "$description" --compatible-architectures "$arch" \
  --compatible-runtimes "${COMPATIBLE_RUNTIMES[@]}" --zip-file "fileb://$zip" --output json)
arn=$(jq -r .LayerVersionArn <<<"$out")
sha=$(jq -r .Content.CodeSha256 <<<"$out")
if [ "$sha" != "$expected" ]; then
  echo "::error::$arn was published with CodeSha256 $sha, expected $expected." >&2
  exit 1
fi
echo "$region $arch: published $arn" >&2
result "$arn" "$sha" false
