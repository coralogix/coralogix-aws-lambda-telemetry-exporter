#!/bin/bash

# Prerequisites:
#  - Installed and configured AWS CLI
# Usage:
# 1. export AWS_PROFILE=<profile name>
# 2. ./sync_layer.sh <source layer ARN> <target layer ARN> <public true/false> <is_target_limited_region true/false>

set -euo pipefail

source_layer_arn=$1
target_layer_arn=$2
public=$3
is_target_limited_region=$4
# shellcheck disable=SC2001
source_region=$(sed 's/arn:aws:lambda:\([^:]*\):[^:]*:[^:]*:\([^:]*\)/\1/' <<< "$source_layer_arn")
# shellcheck disable=SC2001
source_layer_name=$(sed 's/arn:aws:lambda:\([^:]*\):[^:]*:[^:]*:\([^:]*\)/\2/' <<< "$source_layer_arn")
# shellcheck disable=SC2001
target_region=$(sed 's/arn:aws:lambda:\([^:]*\):[^:]*:[^:]*:\([^:]*\)/\1/' <<< "$target_layer_arn")
# shellcheck disable=SC2001
target_layer_name=$(sed 's/arn:aws:lambda:\([^:]*\):[^:]*:[^:]*:\([^:]*\)/\2/' <<< "$target_layer_arn")

source_layer_versions=$(aws lambda list-layer-versions --layer-name "$source_layer_arn" --region "$source_region")
target_layer_versions=$(aws lambda list-layer-versions --layer-name "$target_layer_arn" --region "$target_region")

highest_source_version_number=$(jq '.LayerVersions | max_by(.Version).Version' <<< "$source_layer_versions")
highest_target_version_number=$(jq '.LayerVersions | max_by(.Version).Version // 0' <<< "$target_layer_versions")

echo "highest_source_version_number: $highest_source_version_number highest_target_version_number: $highest_target_version_number"

# Check if all the versions we intend to copy are present at the source
for ((i = (highest_target_version_number + 1); i <= highest_source_version_number; i++)); do
    layer_version=$(jq ".LayerVersions[] | select(.Version == $i)" <<< "$source_layer_versions")
    if [ "$layer_version" = "null" ]; then
        echo "Version $i of the source layer is missing."
        exit 1
    fi
done

for ((i = (highest_target_version_number + 1); i <= highest_source_version_number; i++)); do
    layer_version=$(jq ".LayerVersions[] | select(.Version == $i)" <<< "$source_layer_versions")
    compatible_runtimes=$(jq -r '.CompatibleRuntimes | join(" ")' <<< "$layer_version")
    echo "Syncing layer version $i"

    # Download the source layer
    rm -rf sync_layer_tmp
    mkdir sync_layer_tmp
    pushd sync_layer_tmp > /dev/null

    source_url=$(aws lambda get-layer-version --layer-name "$source_layer_name" --version-number "$i" --query Content.Location --output text --region "$source_region")

    curl -s "$source_url" -o layer.zip

    # Create the target layer
    if [ "$is_target_limited_region" == true ]; then
        # shellcheck disable=SC2086
        output=$(aws lambda publish-layer-version --layer-name "$target_layer_name" --compatible-runtimes $compatible_runtimes --zip-file fileb://layer.zip --region "$target_region")
    else
        compatible_architectures=$(jq -r '(.CompatibleArchitectures // ["x86_64", "arm64"]) | join(" ")' <<< "$layer_version") # arn:aws:lambda:eu-west-1:625240141681:layer:coralogix-opentelemetry-python-wrapper:2 has no compatible architectures, so we need a fallback to work around that
        # shellcheck disable=SC2086
        output=$(aws lambda publish-layer-version --layer-name "$target_layer_name" --compatible-architectures $compatible_architectures --compatible-runtimes $compatible_runtimes --zip-file fileb://layer.zip --region "$target_region")
    fi
    created_version_number=$(jq -r .Version <<< "$output")
    created_version_arn=$(jq -r .LayerVersionArn <<< "$output")
    if [ "$created_version_number" != "$i" ]; then
        echo "Expected to create version $i of $target_layer_arn instead created version $created_version_number!"
        exit 2
    fi
    
    # Make it public if that's expected
    if [ "$public" = true ] ; then
        aws lambda add-layer-version-permission --layer-name "$target_layer_name" --principal '*' --action lambda:GetLayerVersion --version-number "$created_version_number" --statement-id public --region "$target_region" > /dev/null
    fi

    echo "$created_version_arn"
    popd > /dev/null
done

rm -rf sync_layer_tmp
