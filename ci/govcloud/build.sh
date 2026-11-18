#!/usr/bin/env bash
# Verify, test, build and package the GovCloud FIPS layer for the native architecture.
# Runs inside ci/govcloud/Dockerfile.builder with the repo mounted at the working
# directory:
#
#   docker build -t govcloud-builder -f ci/govcloud/Dockerfile.builder ci/govcloud
#   docker run --rm -v "$PWD":/work -w /work govcloud-builder ci/govcloud/build.sh
#
# Output: target/package-<arch>-fips.zip, laid out like the commercial layer.
# It replaces Cargo.lock and .cargo/config.toml with the GovCloud inputs; run it on a
# clean checkout or restore them afterwards.
set -euo pipefail

CARGO_DENY_VERSION=0.20.2
MAX_GLIBC=2.34          # AL2023 Lambda runtimes
FIPS_MODULE="AWS-LC FIPS 3."
FEATURES=(--locked --no-default-features --features fips)

case "$(uname -m)" in
  x86_64)        arch=x86_64; machine="X86-64" ;;
  aarch64|arm64) arch=arm64;  machine="AArch64" ;;
  *) echo "unsupported architecture $(uname -m)" >&2; exit 1 ;;
esac
TARGET="$(uname -m | sed 's/arm64/aarch64/')-unknown-linux-gnu"
export TARGET

# cx_sdk_otlp's build script fetches public protobuf definitions with protofetch,
# which defaults to SSH.
export PROTOFETCH_GIT_PROTOCOL="${PROTOFETCH_GIT_PROTOCOL:-https}"
export PROTOFETCH_GIT_BACKEND="${PROTOFETCH_GIT_BACKEND:-cli}"

group() { echo "::group::$*"; }
endgroup() { echo "::endgroup::"; }

group "toolchain"
if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain none --no-modify-path
fi
rustup show
# test-vendored-webpki.sh works in a clone without rust-toolchain.toml.
rustup default "$(awk -F'"' '/^[[:space:]]*channel[[:space:]]*=/{print $2}' rust-toolchain.toml)"
if [ "$(cargo deny --version 2>/dev/null)" != "cargo-deny $CARGO_DENY_VERSION" ]; then
  curl --proto '=https' --tlsv1.2 -sSfL \
    "https://github.com/EmbarkStudios/cargo-deny/releases/download/${CARGO_DENY_VERSION}/cargo-deny-${CARGO_DENY_VERSION}-$(uname -m)-unknown-linux-musl.tar.gz" \
    | tar -xz --strip-components=1 -C "$CARGO_HOME/bin" "cargo-deny-${CARGO_DENY_VERSION}-$(uname -m)-unknown-linux-musl/cargo-deny"
fi
cargo deny --version
endgroup

group "GovCloud inputs"
mkdir -p .cargo
cp ci/govcloud/config.toml .cargo/config.toml
cp ci/govcloud/Cargo.lock Cargo.lock
endgroup

group "cargo deny"
cargo deny --config ci/govcloud/deny.toml check advisories bans
endgroup

group "build inputs ($TARGET)"
ci/govcloud/verify-build-inputs.sh
endgroup

group "vendored rustls-webpki regression suite"
ci/govcloud/test-vendored-webpki.sh
endgroup

group "clippy and tests"
cargo clippy "${FEATURES[@]}" --all-targets -- -Dwarnings
cargo test "${FEATURES[@]}"
endgroup

group "release build and package"
cargo build --release "${FEATURES[@]}"
rm -rf target/extensions "target/package-$arch-fips.zip"
mkdir -p target/extensions
cp target/release/coralogix-aws-lambda-telemetry-exporter target/extensions/
(cd target && zip -9 "package-$arch-fips.zip" extensions/coralogix-aws-lambda-telemetry-exporter)
endgroup

group "check the packaged binary"
check_dir=$(mktemp -d)
trap 'rm -rf "$check_dir"' EXIT
unzip -q "target/package-$arch-fips.zip" -d "$check_dir"
bin="$check_dir/extensions/coralogix-aws-lambda-telemetry-exporter"

got_machine=$(readelf -h "$bin" | awk -F: '/Machine:/{print $2}')
echo "machine:$got_machine"
[[ "$got_machine" == *"$machine"* ]] || { echo "::error::expected $machine, got$got_machine"; exit 1; }

max_glibc=$(objdump -T "$bin" | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sed 's/GLIBC_//' | sort -uV | tail -1)
echo "highest GLIBC required: ${max_glibc:-none}"
if [ -n "$max_glibc" ] && [ "$(printf '%s\n%s\n' "$MAX_GLIBC" "$max_glibc" | sort -V | tail -1)" != "$MAX_GLIBC" ]; then
  echo "::error::requires GLIBC $max_glibc, above $MAX_GLIBC (AL2023)"; exit 1
fi

module=$(grep -aoE "AWS-LC FIPS [0-9]+\.[0-9]+\.[0-9]+" "$bin" | sort -u)
echo "FIPS module: ${module:-not found}"
[[ "$module" == "$FIPS_MODULE"* ]] && [ "$(wc -l <<<"$module")" = 1 ] \
  || { echo "::error::expected exactly one '${FIPS_MODULE}x' module string"; exit 1; }
endgroup

sha256sum "target/package-$arch-fips.zip"
echo "GovCloud FIPS build OK ($arch, glibc <= $MAX_GLIBC, $module)"
