#!/bin/bash

set -euo pipefail

# Expects these env vars:
# ARCHITECTURE "x86_64" / "arm64"
# RUNTIME "nodejs" / "python" / "java"
# Plus the vars set by set_*_vars.sh

publish_in_regular_regions () {
    local NAME="$1"
    local ARCH="$2"
    local RUNTIMES="$3"

    for region in ${REGULAR_REGIONS[@]}; do 
        local output=`aws lambda publish-layer-version --layer-name $NAME --compatible-architectures $ARCH --compatible-runtimes $RUNTIMES --zip-file fileb://target/$NAME.zip --region $region`
        local version=`echo "$output" | jq -r .Version` 
        local versionArn=`echo "$output" | jq -r .LayerVersionArn`
        if [ "$PUBLIC" = true ] ; then
            aws lambda add-layer-version-permission --layer-name $NAME --principal '*' --action lambda:GetLayerVersion --version-number $version --statement-id public --region $region > /dev/null
        fi
        echo $versionArn
    done
}

publish_in_limited_regions () {
    local NAME="$1"
    local RUNTIMES="$2"

    for region in ${LIMITED_REGIONS[@]}; do 
        local output=`aws lambda publish-layer-version --layer-name $NAME --compatible-runtimes $RUNTIMES --zip-file fileb://target/$NAME.zip --region $region`
        local version=`echo "$output" | jq -r .Version` 
        local versionArn=`echo "$output" | jq -r .LayerVersionArn`
        if [ "$PUBLIC" = true ] ; then
            aws lambda add-layer-version-permission --layer-name $NAME --principal '*' --action lambda:GetLayerVersion --version-number $version --statement-id public --region $region > /dev/null
        fi
        echo $versionArn
    done
}

COMBINED_LAYER_NAME="coralogix-${RUNTIME}-wrapper-and-exporter-${ARCHITECTURE}${NAME_SUFFIX}"
if [ "$RUNTIME" = "nodejs" ] ; then
    SUPPORTED_RUNTIMES="nodejs22.x nodejs24.x nodejs26.x"
elif [ "$RUNTIME" = "python" ] ; then
    SUPPORTED_RUNTIMES="python3.9 python3.10 python3.11 python3.12 python3.13 python3.14"
elif [ "$RUNTIME" = "java" ] ; then
    SUPPORTED_RUNTIMES="java8.al2 java11 java17 java21"
fi

if [ "$ARCHITECTURE" = "x86_64" ] ; then
    publish_in_regular_regions "$COMBINED_LAYER_NAME" "$ARCHITECTURE" "$SUPPORTED_RUNTIMES"
    publish_in_limited_regions "$COMBINED_LAYER_NAME" "$SUPPORTED_RUNTIMES"
else # amd64
    publish_in_regular_regions "$COMBINED_LAYER_NAME" "$ARCHITECTURE" "$SUPPORTED_RUNTIMES"
fi
