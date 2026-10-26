#!/bin/bash

# Prerequisites:
#  - Installed and configured AWS CLI
# Usage:
#  ./check_layer_version.sh <layer name> <region> [<region> ...]
# Prints the latest version on stdout if it is the same in all regions, otherwise exits with an error.

set -euo pipefail

if [ "$#" -lt 2 ]; then
    echo "Usage: $0 <layer name> <region> [<region> ...]"
    exit 1
fi

layer_name=$1
shift
regions=("$@")

# Prints the latest version of the layer in the region (versions are listed newest first), or 0 if the layer does not exist.
get_latest_version () {
    local output

    if ! output=$(aws lambda list-layer-versions --layer-name "$layer_name" --region "$1" --query 'LayerVersions[0].Version' --output text 2>&1); then
        if [[ "$output" == *ResourceNotFoundException* ]]; then
            echo 0
            return
        fi
        echo "Failed to list layer versions for $layer_name in $1: $output" >&2
        exit 1
    fi

    if [ "$output" = "None" ] || [ -z "$output" ]; then
        echo 0
    else
        echo "$output"
    fi
}

versions=()
consistent=true

for region in "${regions[@]}"; do
    version=$(get_latest_version "$region")
    versions+=("$version")
    if [ "$version" != "${versions[0]}" ]; then
        consistent=false
    fi
done

if [ "$consistent" = true ]; then
    echo "Layer $layer_name: latest version ${versions[0]} is consistent across ${#regions[@]} regions." >&2
    echo "${versions[0]}"
    exit 0
fi

echo "Layer $layer_name: latest version is not consistent across regions." >&2
echo "region version" >&2
for i in "${!regions[@]}"; do
    echo "${regions[$i]} ${versions[$i]}" >&2
done
echo "Align layer versions across regions (e.g. sync-region workflow) before publishing." >&2
exit 1
