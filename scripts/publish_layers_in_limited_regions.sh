#!/bin/bash

set -euo pipefail

# The number of runtimes allowed here is limited. To add a new runtime, you must remove an existing one.
COMPATIBLE_RUNTIMES="nodejs22.x nodejs24.x nodejs26.x java8.al2 java11 java17 java21 python3.10 python3.11 python3.12 python3.13 python3.14 dotnet8 provided.al2 provided.al2023"

for region in ${REGIONS[@]}; do
    output=`aws lambda publish-layer-version --layer-name coralogix-aws-lambda-telemetry-exporter-x86_64${NAME_SUFFIX} --compatible-runtimes $COMPATIBLE_RUNTIMES --zip-file fileb://target/package-x86_64.zip --region $region`
    version=`echo "$output" | jq -r .Version` 
    versionArn=`echo "$output" | jq -r .LayerVersionArn`
    if [ "$PUBLIC" = true ] ; then
        aws lambda add-layer-version-permission --layer-name coralogix-aws-lambda-telemetry-exporter-x86_64${NAME_SUFFIX} --principal '*' --action lambda:GetLayerVersion --version-number $version --statement-id public --region $region > /dev/null
    fi
    echo $versionArn
done