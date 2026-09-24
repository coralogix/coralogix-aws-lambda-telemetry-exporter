#!/bin/bash

set -euo pipefail

mkdir -p "${BASH_SOURCE%/*}/../docker/amd64"
pushd "${BASH_SOURCE%/*}/../docker/amd64" > /dev/null
unzip ../../target/$1
popd > /dev/null

mkdir -p "${BASH_SOURCE%/*}/../docker/arm64"
pushd "${BASH_SOURCE%/*}/../docker/arm64" > /dev/null
unzip ../../target/$2
popd > /dev/null
