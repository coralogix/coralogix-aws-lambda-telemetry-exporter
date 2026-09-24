#!/bin/bash

set -euo pipefail

AWS_LC_FIPS_SYS_CMAKE_TOOLCHAIN_FILE_x86_64_unknown_linux_gnu="$PWD/scripts/cmake/x86_64-toolchain.cmake" \
  TARGET_CC=x86_64-unknown-linux-gnu-gcc \
  cargo lambda build --extension --release --locked \
  --no-default-features --features fips \
  --target x86_64-unknown-linux-gnu.2.26

pushd target

rm -rf extensions
mkdir extensions

cp lambda/extensions/coralogix-aws-lambda-telemetry-exporter extensions
zip -9 package-x86_64-fips.zip extensions/*

popd
