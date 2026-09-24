#!/bin/bash

set -euo pipefail

# Disabling jitter entropy to avoid slowness during the first use of the crypto provider
AWS_LC_SYS_NO_JITTER_ENTROPY=1 \
  TARGET_CC=x86_64-unknown-linux-gnu-gcc \
  cargo lambda build --extension --release --locked \
  --target x86_64-unknown-linux-gnu.2.26

pushd target

rm -rf extensions
mkdir extensions

cp lambda/extensions/coralogix-aws-lambda-telemetry-exporter extensions
zip -9 package-x86_64.zip extensions/* 

popd
