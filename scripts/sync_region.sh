#!/bin/bash

# Prerequisites:
#  - Installed and configured AWS CLI
# Usage:
# 1. export AWS_PROFILE=<profile name>
# 2. ./sync_region.sh <source account> <source region> <target account> <target region> <name suffix> <public true/false> <is_target_limited_region true/false>

set -euo pipefail

source_account=$1
source_region=$2
target_account=$3
target_region=$4
suffix=$5
public=$6
is_target_limited_region=$7

if [ "$is_target_limited_region" != true ]; then
    ./scripts/sync_layer.sh "arn:aws:lambda:${source_region}:${source_account}:layer:coralogix-aws-lambda-telemetry-exporter-arm64${suffix}" "arn:aws:lambda:${target_region}:${target_account}:layer:coralogix-aws-lambda-telemetry-exporter-arm64${suffix}" "$public" "$is_target_limited_region"
    ./scripts/sync_layer.sh "arn:aws:lambda:${source_region}:${source_account}:layer:coralogix-nodejs-wrapper-and-exporter-arm64${suffix}" "arn:aws:lambda:${target_region}:${target_account}:layer:coralogix-nodejs-wrapper-and-exporter-arm64${suffix}" "$public" "$is_target_limited_region"
    ./scripts/sync_layer.sh "arn:aws:lambda:${source_region}:${source_account}:layer:coralogix-python-wrapper-and-exporter-arm64${suffix}" "arn:aws:lambda:${target_region}:${target_account}:layer:coralogix-python-wrapper-and-exporter-arm64${suffix}" "$public" "$is_target_limited_region"
    ./scripts/sync_layer.sh "arn:aws:lambda:${source_region}:${source_account}:layer:coralogix-java-wrapper-and-exporter-arm64${suffix}" "arn:aws:lambda:${target_region}:${target_account}:layer:coralogix-java-wrapper-and-exporter-arm64${suffix}" "$public" "$is_target_limited_region"
fi

./scripts/sync_layer.sh "arn:aws:lambda:${source_region}:${source_account}:layer:coralogix-aws-lambda-telemetry-exporter-x86_64${suffix}" "arn:aws:lambda:${target_region}:${target_account}:layer:coralogix-aws-lambda-telemetry-exporter-x86_64${suffix}" "$public" "$is_target_limited_region"
./scripts/sync_layer.sh "arn:aws:lambda:${source_region}:${source_account}:layer:coralogix-nodejs-wrapper-and-exporter-x86_64${suffix}" "arn:aws:lambda:${target_region}:${target_account}:layer:coralogix-nodejs-wrapper-and-exporter-x86_64${suffix}" "$public" "$is_target_limited_region"
./scripts/sync_layer.sh "arn:aws:lambda:${source_region}:${source_account}:layer:coralogix-python-wrapper-and-exporter-x86_64${suffix}" "arn:aws:lambda:${target_region}:${target_account}:layer:coralogix-python-wrapper-and-exporter-x86_64${suffix}" "$public" "$is_target_limited_region"
./scripts/sync_layer.sh "arn:aws:lambda:${source_region}:${source_account}:layer:coralogix-java-wrapper-and-exporter-x86_64${suffix}" "arn:aws:lambda:${target_region}:${target_account}:layer:coralogix-java-wrapper-and-exporter-x86_64${suffix}" "$public" "$is_target_limited_region"
./scripts/sync_layer.sh "arn:aws:lambda:${source_region}:${source_account}:layer:coralogix-opentelemetry-nodejs-wrapper${suffix}" "arn:aws:lambda:${target_region}:${target_account}:layer:coralogix-opentelemetry-nodejs-wrapper${suffix}" "$public" "$is_target_limited_region"
./scripts/sync_layer.sh "arn:aws:lambda:${source_region}:${source_account}:layer:coralogix-opentelemetry-python-wrapper${suffix}" "arn:aws:lambda:${target_region}:${target_account}:layer:coralogix-opentelemetry-python-wrapper${suffix}" "$public" "$is_target_limited_region"
./scripts/sync_layer.sh "arn:aws:lambda:${source_region}:${source_account}:layer:coralogix-opentelemetry-java-wrapper${suffix}" "arn:aws:lambda:${target_region}:${target_account}:layer:coralogix-opentelemetry-java-wrapper${suffix}" "$public" "$is_target_limited_region"
