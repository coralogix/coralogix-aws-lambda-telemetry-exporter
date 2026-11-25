#!/usr/bin/env bash
# Tests the GovCloud combined-layer helpers and the script hooks they use, against a stub
# `aws` CLI, without AWS.
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"

# Stub aws, logging every call to $STUB_CALLS. STUB_PRIVATE lists "<layer>:<version>:<region>"
# entries whose policy has no public statement, STUB_ORG entries shared with an organization
# only; STUB_CONFLICT makes every grant fail as if already granted.
cat > "$work/bin/aws" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_CALLS"
arg() { local want=$1; shift; while [ $# -gt 0 ]; do [ "$1" = "$want" ] && { echo "$2"; return; }; shift; done; }
case "$1 $2" in
  "lambda list-layer-versions") echo "${STUB_VERSIONS:-3 2 1}" ;;
  "lambda get-layer-version-policy")
    key="$(arg --layer-name "$@"):$(arg --version-number "$@"):$(arg --region "$@")"
    if [[ " ${STUB_PRIVATE:-} " == *" $key "* ]]; then echo '{"Statement":[]}'
    elif [[ " ${STUB_ORG:-} " == *" $key "* ]]; then
      echo '{"Statement":[{"Effect":"Allow","Principal":"*","Action":"lambda:GetLayerVersion","Condition":{"StringEquals":{"aws:PrincipalOrgID":"o-x"}}}]}'
    else echo '{"Statement":[{"Effect":"Allow","Principal":"*","Action":"lambda:GetLayerVersion"}]}'; fi ;;
  "lambda add-layer-version-permission")
    [ -n "${STUB_CONFLICT:-}" ] && { echo "ResourceConflictException" >&2; exit 254; } ;;
  "lambda publish-layer-version") echo '{"Version":1,"LayerVersionArn":"arn:x:1"}' ;;
  *) echo "stub aws: unexpected $*" >&2; exit 1 ;;
esac
EOF
chmod +x "$work/bin/aws"
export PATH="$work/bin:$PATH" STUB_CALLS="$work/calls"

fail=0
check() { # check <name> <command...>: passes when the command succeeds
  local name=$1; shift
  if "$@" >"$work/out" 2>&1; then echo "ok    $name"; else echo "FAIL  $name"; cat "$work/out"; fail=1; fi
}
x=coralogix-aws-lambda-telemetry-exporter-x86_64 a=coralogix-aws-lambda-telemetry-exporter-arm64

# public-exporter-version.sh
check "picks the newest public exporter version, skipping a private newer one" \
  bash -c "[ \"\$(STUB_PRIVATE='$a:3:us-gov-east-1' '$here/public-exporter-version.sh' 2>/dev/null)\" = 2 ]"
check "skips a version shared with an organization only" \
  bash -c "[ \"\$(STUB_ORG='$x:3:us-gov-west-1' '$here/public-exporter-version.sh' 2>/dev/null)\" = 2 ]"
check "fails when no version is public everywhere" \
  bash -c "! STUB_VERSIONS='1' STUB_PRIVATE='$x:1:us-gov-west-1' '$here/public-exporter-version.sh'"

# ensure-public.sh
check "accepts a version that is already public" \
  env STUB_CONFLICT=1 "$here/ensure-public.sh" layer-x 4
check "fails when a region stays private" \
  bash -c "! STUB_PRIVATE='layer-x:4:us-gov-east-1' '$here/ensure-public.sh' layer-x 4"
check "fails when a region is shared with an organization only" \
  bash -c "! STUB_CONFLICT=1 STUB_ORG='layer-x:4:us-gov-west-1' '$here/ensure-public.sh' layer-x 4"

# merge_layers.sh with explicit zips: no download, both layers in the result.
mkdir -p "$work/scripts" "$work/e" "$work/w"
cp "$repo/scripts/merge_layers.sh" "$work/scripts/"
echo exporter > "$work/e/extension" && (cd "$work/e" && zip -q ../exporter.zip extension)
echo wrapper > "$work/w/otel-instrument" && (cd "$work/w" && zip -q ../wrapper.zip otel-instrument)
: > "$work/calls"
check "merges the given zips without downloading" bash -c "
  ARCHITECTURE=arm64 RUNTIME=python NAME_SUFFIX='' EXPORTER_ZIP='$work/exporter.zip' WRAPPER_ZIP='$work/wrapper.zip'
  source '$work/scripts/merge_layers.sh' >/dev/null
  unzip -Z1 '$work/target/coralogix-python-wrapper-and-exporter-arm64.zip' > '$work/listing'
  [ ! -s '$work/calls' ] && grep -qx extension '$work/listing' && grep -qx otel-instrument '$work/listing'"

# publish_combined_layer.sh runtime lists.
cp "$repo/scripts/publish_combined_layer.sh" "$work/scripts/"
publish() { # publish <runtime> [VAR=value...]: prints the runtimes it published with
  : > "$work/calls"
  env ${2:+"${@:2}"} bash -c "REGULAR_REGIONS=(us-gov-west-1); LIMITED_REGIONS=(); PUBLIC=false; NAME_SUFFIX=''
    RUNTIME=$1 ARCHITECTURE=arm64; cd '$work' && source scripts/publish_combined_layer.sh" >/dev/null
  sed -n 's/.*--compatible-runtimes \(.*\) --zip-file.*/\1/p' "$work/calls"
}
same() { # same <name> <actual> <expected>
  if [ "$2" = "$3" ]; then echo "ok    $1"; else echo "FAIL  $1: got '$2', want '$3'"; fail=1; fi
}
same "GovCloud publishes AL2023 Python runtimes only" \
  "$(publish python PYTHON_COMBINED_RUNTIMES='python3.12 python3.13 python3.14')" "python3.12 python3.13 python3.14"
same "commercial keeps its Python runtimes" "$(publish python)" "python3.9 python3.10 python3.11 python3.12 python3.13 python3.14"
same "commercial keeps its Java runtimes" "$(publish java)" "java8.al2 java11 java17 java21"
check "set_govcloud_vars.sh narrows every wrapper runtime to AL2023" bash -c "
  source '$repo/scripts/set_govcloud_vars.sh'
  [ \"\$PYTHON_COMBINED_RUNTIMES\" = 'python3.12 python3.13 python3.14' ] && [ \"\$NODEJS_COMBINED_RUNTIMES\" = 'nodejs22.x nodejs24.x nodejs26.x' ] && [ \"\$JAVA_COMBINED_RUNTIMES\" = java21 ]"

exit "$fail"
