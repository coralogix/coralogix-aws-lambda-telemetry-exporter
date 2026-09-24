#!/bin/bash

set -euo pipefail

layer_name=$1
source_region=$2
destination_region=$3

reverse_array() {
  local arr=("$@")
  local len=${#arr[@]}
  local reversed_arr=()

  for (( i=len-1; i>=0; i-- )); do
    reversed_arr+=("${arr[i]}")
  done

  echo "${reversed_arr[@]}"
}

versions=($(aws lambda list-layer-versions --layer-name $layer_name --region $source_region --query "LayerVersions[].Version" --output text))
versions=($(reverse_array "${versions[@]}"))
existing_destination_versions=($(aws lambda list-layer-versions --layer-name $layer_name --region $destination_region --query "LayerVersions[].Version" --output text))

echo "Going to copy versions: ${versions[*]} of $layer_name from $source_region to $destination_region"

if [ "${#existing_destination_versions[@]}" -gt 0 ]; then
    echo "The destination region contains versions: ${existing_destination_versions[*]}"
    echo "There are layer version in destination region! Aborting."
    exit 0
fi

read -p "Do you want to proceed? (y/n): " confirm

if [ "$confirm" != "y" ] && [ "$confirm" != "Y" ]; then
    echo "Aborted."
    exit 0
fi

for version in "${versions[@]}"; do
    echo "Copying version $version..."
    
    layer_version=$(aws lambda get-layer-version --layer-name $layer_name --version-number $version --region $source_region)
    url="$(echo $layer_version | jq -r '.Content.Location')"
    curl -s $url -o target/copied-layer.zip

    compatible_runtimes="$(echo $layer_version | jq -r '.CompatibleRuntimes | join(" ")')"
    echo $compatible_runtimes

    destination_layer_version=$(aws lambda publish-layer-version --layer-name $layer_name \
                                                                  --description "$(echo $layer_version | jq -r '.Description')" \
                                                                  --zip-file "fileb://target/copied-layer.zip" \
                                                                  --compatible-runtimes $compatible_runtimes \
                                                                  --license-info "$(echo $layer_version | jq -r '.LicenseInfo')" \
                                                                  --region $destination_region)
    
    echo "Copied version $version to $destination_region"
done

echo "Done"
