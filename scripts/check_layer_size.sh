#!/bin/bash

# Prerequisites:
#  - Installed and configured AWS CLI
# Usage:
# 1. export AWS_PROFILE=<profile name>
# 2. ./check_layer_size.sh <layer version ARN>

set -euo pipefail

region=$(echo $1 | sed 's/\(arn:aws:lambda:\([^:]*\):[^:]*:[^:]*:[^:]*\):\([^:]*\)/\2/')
layer=$(echo $1 | sed 's/\(arn:aws:lambda:\([^:]*\):[^:]*:[^:]*:[^:]*\):\([^:]*\)/\1/')
layer_version=$(echo $1 | sed 's/\(arn:aws:lambda:\([^:]*\):[^:]*:[^:]*:[^:]*\):\([^:]*\)/\3/')

url=$(aws lambda get-layer-version --layer-name $layer --version-number $layer_version --query Content.Location --output text --region $region)

rm -rf check_size_output
mkdir check_size_output
pushd check_size_output > /dev/null

curl -s $url -o layer.zip
unzip -q layer.zip -d unzipped

zipped_size_kb=$(du -d 0 -k "layer.zip" | cut -f1)
zipped_size_b=$((zipped_size_kb * 1024))
unzipped_size_kb=$(du -d 0 -k "unzipped" | cut -f1)
unzipped_size_b=$((unzipped_size_kb * 1024))

printf "Zipped size %'dB, unzipped size %'dB\n" $zipped_size_b $unzipped_size_b

popd > /dev/null
