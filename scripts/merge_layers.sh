#!/bin/bash

set -euo pipefail

# Expects these env vars:
# ARCHITECTURE   "x86_64" / "arm64"
# RUNTIME "nodejs" / "python"
# Plus the vars set by set_*_vars.sh

download_layer () {
    local LAYER_NAME="$1"

    local VERSION=`aws lambda list-layer-versions --layer-name "$LAYER_NAME" --query 'LayerVersions[0].Version'`
    local URL=$(aws lambda get-layer-version --layer-name "$LAYER_NAME" --version-number "$VERSION" --query Content.Location --output text)
    echo "Downloading layer ${LAYER_NAME} version ${VERSION}"
    curl "$URL" -o "${LAYER_NAME}.zip"
}

merge_zips () {
    local ZIP_FILE1="$1"
    local ZIP_FILE2="$2"
    local OUTPUT_FILE="$3"

    rm -rf "$OUTPUT_FILE"
    rm -rf "zip-tmp"
    mkdir -p "zip-tmp"
    unzip -q -d "zip-tmp" "$ZIP_FILE1"
    rm -rf "zip-tmp/META_INF" # remove the signature file (boths zips have it so it would be a conflict of file names)
    unzip -q -d "zip-tmp" "$ZIP_FILE2"
    rm -rf "zip-tmp/META_INF" # remove the signature file (boths zips have it so it would be a conflict of file names)
    pushd zip-tmp > /dev/null
    zip -q -r "../$OUTPUT_FILE" "."
    popd > /dev/null
}

mkdir -p "${BASH_SOURCE%/*}/../target"
pushd "${BASH_SOURCE%/*}/../target" > /dev/null

EXPORTER_LAYER_NAME="coralogix-aws-lambda-telemetry-exporter-${ARCHITECTURE}${NAME_SUFFIX}"
WRAPPER_LAYER_NAME="coralogix-opentelemetry-${RUNTIME}-wrapper${NAME_SUFFIX}"
COMBINED_LAYER_NAME="coralogix-${RUNTIME}-wrapper-and-exporter-${ARCHITECTURE}${NAME_SUFFIX}"

download_layer "$EXPORTER_LAYER_NAME"
download_layer "$WRAPPER_LAYER_NAME"
merge_zips "$EXPORTER_LAYER_NAME.zip" "$WRAPPER_LAYER_NAME.zip" "$COMBINED_LAYER_NAME"

popd > /dev/null
