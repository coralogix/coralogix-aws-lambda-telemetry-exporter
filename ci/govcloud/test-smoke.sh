#!/usr/bin/env bash
# Tests ci/govcloud/smoke.sh against a stub `aws` CLI, without AWS or Coralogix.
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
layer=arn:aws-us-gov:lambda:us-gov-west-1:123456789012:layer:x:1

# Stub aws, logging every call to $STUB_CALLS. STUB_* variables choose the behaviour.
cat > "$work/bin/aws" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_CALLS"
case "$1 $2" in
  "secretsmanager describe-secret") echo "arn:aws-us-gov:secretsmanager:us-gov-west-1:123456789012:secret:key" ;;
  "lambda create-function")
    if [ -n "${STUB_CREATE_FAILS:-}" ]; then echo "Read timeout on endpoint URL" >&2; exit 255; fi ;;
  "lambda wait") ;;
  "lambda invoke")
    out=${!#}
    if [ -n "${STUB_FUNCTION_ERROR:-}" ]; then
      printf '{"errorMessage":"boom"}' > "$out"; echo '{"StatusCode":200,"FunctionError":"Unhandled"}'
    else
      printf '"ok"' > "$out"; echo '{"StatusCode":200}'
    fi ;;
  "logs filter-log-events")
    if [ -n "${STUB_NO_FIPS:-}" ]; then echo '{"events":[]}'; else
      echo '{"events":[{"message":"coralogix:  INFO Starting coralogix-aws-lambda-telemetry-exporter 1.3.0 FIPS"}]}'; fi ;;
  "lambda delete-function"|"logs delete-log-group")
    if [ -n "${STUB_DELETE_FAILS:-}" ]; then echo "An error occurred (ServiceUnavailable)" >&2; exit 254; fi ;;
  *) echo "stub aws: unexpected $*" >&2; exit 1 ;;
esac
EOF
# The marker is random, so the query echoes back whatever smoke.sh logged as the marker.
cat > "$work/bin/query" <<'EOF'
#!/usr/bin/env bash
grep -o 'marker: .*' "$SMOKE_LOG" | head -1 | cut -d' ' -f2-
EOF
printf '#!/bin/sh\n' > "$work/bin/sleep"
# The clock jumps 5 minutes per read, so a failing run gives up after a few polls
# instead of waiting out the 15-minute deadline.
cat > "$work/bin/date" <<'EOF'
#!/bin/sh
[ "$1" = "+%s" ] || exec /bin/date "$@"
n=$(cat "$STUB_CLOCK" 2>/dev/null || echo 0); echo $((n + 300)) > "$STUB_CLOCK"; echo "$n"
EOF
chmod +x "$work/bin/"*

fail=0
run() { # run <name> <expected exit> [VAR=value...]
  local name=$1 want=$2; shift 2
  local log="$work/$name.log" got
  rm -f "$work/clock" "$work/calls"
  env PATH="$work/bin:$PATH" STUB_CALLS="$work/calls" STUB_CLOCK="$work/clock" SMOKE_DOMAIN=example.invalid \
    SMOKE_QUERY_CMD=query SMOKE_LOG="$log" SMOKE_RUN_ID=42 "$@" \
    bash "$here/smoke.sh" "$layer" arm64 >"$log" 2>&1
  got=$?
  if [ "$got" = "$want" ]; then echo "ok    $name"; else echo "FAIL  $name (exit $got, want $want)"; cat "$log"; fail=1; fi
}
expect() { # expect <name> <file> <text>
  if grep -q -- "$3" "$2"; then echo "ok    $1"; else echo "FAIL  $1"; cat "$2"; fail=1; fi
}

run "passes when FIPS, delivery and cleanup all succeed" 0
expect "runs as the existing smoke role" "$work/calls" "--role arn:aws-us-gov:iam::123456789012:role/smoke-exporter "
expect "attaches the layer" "$work/calls" "--layers $layer"
if grep -q "^iam " "$work/calls"; then echo "FAIL  never calls IAM"; fail=1; else echo "ok    never calls IAM"; fi
expect "removes the function" "$work/calls" "lambda delete-function --function-name smoke-exporter-42-arm64"
run "fails on a function error" 1 STUB_FUNCTION_ERROR=1
expect "reports the function error" "$work/fails on a function error.log" "FunctionError=Unhandled"
run "fails without a FIPS startup line" 1 STUB_NO_FIPS=1
expect "says so" "$work/fails without a FIPS startup line.log" "No FIPS startup line"
run "fails a passing run when cleanup leaves resources" 1 STUB_DELETE_FAILS=1
expect "names the leftovers" "$work/fails a passing run when cleanup leaves resources.log" "cleanup left: function"
run "fails when create-function reports an error" 1 STUB_CREATE_FAILS=1
expect "still deletes the function, which AWS may have created" "$work/calls" "lambda delete-function --function-name smoke-exporter-42-arm64"
if env PATH="$work/bin:$PATH" SMOKE_DOMAIN=example.invalid SMOKE_QUERY_CMD=query \
    bash "$here/smoke.sh" not-an-arn arm64 >/dev/null 2>&1; then
  echo "FAIL  rejects a bad layer ARN"; fail=1
else
  echo "ok    rejects a bad layer ARN"
fi

exit "$fail"
