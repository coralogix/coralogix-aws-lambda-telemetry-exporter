#!/usr/bin/env bash
# Tests ci/govcloud/smoke.sh against a stub `aws` CLI, without AWS or Coralogix.
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"

# Stub aws. STUB_* variables choose the behaviour per case.
cat > "$work/bin/aws" <<'EOF'
#!/usr/bin/env bash
case "$1 $2" in
  "sts get-caller-identity") echo '{"Account":"123456789012","Arn":"arn:aws-us-gov:iam::123456789012:user/ci"}' ;;
  "secretsmanager describe-secret") echo "arn:aws-us-gov:secretsmanager:us-gov-west-1:123456789012:secret:key" ;;
  "iam create-role"|"iam put-role-policy"|"lambda create-function"|"lambda wait") ;;
  "lambda invoke")
    out=${!#}
    if [ -n "${STUB_FUNCTION_ERROR:-}" ]; then
      printf '{"errorMessage":"boom"}' > "$out"; echo '{"StatusCode":200,"FunctionError":"Unhandled"}'
    else
      printf '"ok"' > "$out"; echo '{"StatusCode":200}'
    fi ;;
  "logs filter-log-events")
    echo '{"events":[{"message":"coralogix:  INFO Starting coralogix-aws-lambda-telemetry-exporter 1.3.0 FIPS"}]}' ;;
  "lambda delete-function"|"logs delete-log-group"|"iam delete-role-policy"|"iam delete-role")
    if [ -n "${STUB_DELETE_FAILS:-}" ]; then echo "An error occurred (ServiceUnavailable)" >&2; exit 254; fi ;;
  *) echo "stub aws: unexpected $*" >&2; exit 1 ;;
esac
EOF
# The marker is random, so the query echoes back whatever smoke.sh logged as the marker.
cat > "$work/bin/query" <<'EOF'
#!/usr/bin/env bash
[ -n "${STUB_NO_DELIVERY:-}" ] && { echo '[]'; exit 0; }
grep -o 'marker: .*' "$SMOKE_LOG" | head -1 | cut -d' ' -f2-
EOF
printf '#!/bin/sh\n' > "$work/bin/sleep"
chmod +x "$work/bin/"*

fail=0
run() { # run <name> <expected exit> [VAR=value...]
  local name=$1 want=$2; shift 2
  local log="$work/$name.log" got
  env PATH="$work/bin:$PATH" SMOKE_DOMAIN=example.invalid SMOKE_QUERY_CMD=query SMOKE_LOG="$log" "$@" \
    bash "$here/smoke.sh" arn:aws-us-gov:lambda:us-gov-west-1:123456789012:layer:x:1 us-gov-west-1 arm64 >"$log" 2>&1
  got=$?
  if [ "$got" = "$want" ]; then echo "ok    $name"; else echo "FAIL  $name (exit $got, want $want)"; cat "$log"; fail=1; fi
}

run "passes when FIPS, delivery and cleanup all succeed" 0
run "fails on a function error" 1 STUB_FUNCTION_ERROR=1
run "fails a passing run when cleanup leaves resources" 1 STUB_DELETE_FAILS=1
if grep -q "FunctionError=Unhandled" "$work/fails on a function error.log"; then
  echo "ok    reports the function error"
else
  echo "FAIL  reports the function error"; fail=1
fi
if grep -q "cleanup left: role" "$work/fails a passing run when cleanup leaves resources.log"; then
  echo "ok    names the leftovers"
else
  echo "FAIL  names the leftovers"; fail=1
fi
if env PATH="$work/bin:$PATH" SMOKE_DOMAIN=example.invalid SMOKE_QUERY_CMD=query \
    bash "$here/smoke.sh" arn:aws-us-gov:lambda:us-gov-east-1:123456789012:layer:x:1 us-gov-west-1 arm64 >/dev/null 2>&1; then
  echo "FAIL  rejects a layer ARN from another region"; fail=1
else
  echo "ok    rejects a layer ARN from another region"
fi

exit "$fail"
