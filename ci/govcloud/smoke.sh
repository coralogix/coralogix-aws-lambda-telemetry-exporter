#!/usr/bin/env bash
# Smoke-tests a published (private) GovCloud exporter layer version: attaches it to a
# temporary python3.12 function, invokes the function with a unique marker, and passes
# only when the exporter starts in FIPS mode, logs no errors and the marker reaches the
# Coralogix test team.
#
#   ci/govcloud/smoke.sh <layer-version-arn> <region> <arm64|x86_64>
#
# Environment:
#   SMOKE_DOMAIN        Coralogix domain of the test team, e.g. eu2.coralogix.com (required)
#   SMOKE_QUERY_KEY     Coralogix API key with DataQuerying permissions for that team, or
#   SMOKE_QUERY_CMD     a command run as `$SMOKE_QUERY_CMD "<dataprime query>"` that prints
#                       the matching logs (for local runs, e.g. with the cx CLI)
#   SMOKE_SECRET_NAME   Secrets Manager secret with the test team's Send-Your-Data key
#                       (default coralogix-gov-smoke-test-api-key)
#   SMOKE_BOUNDARY_ARN  permissions boundary for the role this creates (optional)
#   SMOKE_RUN_ID        unique id for resource names (default GITHUB_RUN_ID or the time)
#
# Every resource is named smoke-* and removed at the end, pass or fail. The exit code is
# the test's; cleanup problems are reported but never hide a test failure.
set -uo pipefail

usage="usage: smoke.sh <layer-version-arn> <region> <arm64|x86_64>"
layer_arn=${1:?$usage} region=${2:?$usage} arch=${3:?$usage}
case "$arch" in arm64|x86_64) ;; *) echo "::error::arch must be arm64 or x86_64, not '$arch'."; exit 2 ;; esac
[[ $layer_arn =~ ^arn:aws[a-z-]*:lambda:$region:[0-9]{12}:layer:[A-Za-z0-9_-]+:[0-9]+$ ]] \
  || { echo "::error::'$layer_arn' is not a layer version ARN in $region."; exit 2; }
: "${SMOKE_DOMAIN:?SMOKE_DOMAIN is required}"
if [ -z "${SMOKE_QUERY_KEY:-}" ] && [ -z "${SMOKE_QUERY_CMD:-}" ]; then
  echo "::error::Set SMOKE_QUERY_KEY or SMOKE_QUERY_CMD to read the marker back from Coralogix."
  exit 2
fi

export AWS_REGION="$region" AWS_DEFAULT_REGION="$region"
secret_name=${SMOKE_SECRET_NAME:-coralogix-gov-smoke-test-api-key}
run_id=${SMOKE_RUN_ID:-${GITHUB_RUN_ID:-$(date +%s)}}
name="smoke-exporter-${run_id}-${arch}"
name=${name:0:64}
subsystem="$name"
marker="cxexpsmoke ${arch} ${run_id} $(od -An -N6 -tx1 /dev/urandom | tr -d ' \n')"
log() { echo "[$(date -u +%H:%M:%S)] $*"; }
now_ms() { python3 -c 'import time; print(int(time.time() * 1000))'; }

result=1 role_created=false fn_created=false
tmp=$(mktemp -d)
# Before the function exists: Lambda may initialize it, and log the FIPS startup line,
# before the first invocation.
start_ms=$(now_ms)

# Runs "$@" up to three times; succeeds if any attempt does or the resource is already
# gone. Prints nothing on success.
delete() {
  local attempt out
  for attempt in 1 2 3; do
    if out=$("$@" 2>&1); then return 0; fi
    grep -qE 'ResourceNotFoundException|NoSuchEntity' <<<"$out" && return 0
    [ "$attempt" -lt 3 ] && sleep $((attempt * 5))
  done
  echo "$out" | tail -n1
  return 1
}

cleanup() {
  local leftovers=()
  log "cleanup"
  if $fn_created; then
    delete aws lambda delete-function --function-name "$name" || leftovers+=("function $name")
  fi
  # Deleted without checking first: listing log groups needs wider permissions.
  delete aws logs delete-log-group --log-group-name "/aws/lambda/$name" || leftovers+=("log group /aws/lambda/$name")
  if $role_created; then
    delete aws iam delete-role-policy --role-name "$name" --policy-name smoke || leftovers+=("policy smoke on role $name")
    delete aws iam delete-role --role-name "$name" || leftovers+=("role $name")
  fi
  rm -rf "$tmp"
  # Leftovers fail a passing run, so they get noticed; a test failure stays the reason.
  if [ "${#leftovers[@]}" -gt 0 ]; then
    for item in "${leftovers[@]}"; do echo "::error::smoke cleanup left: $item"; done
    result=1
  fi
  log "result: $([ "$result" -eq 0 ] && echo PASS || echo FAIL)"
  exit "$result"
}
trap cleanup EXIT
trap 'exit 1' INT TERM
fail() { echo "::error::$*"; result=1; exit 1; }

caller=$(aws sts get-caller-identity --output json) || fail "No AWS credentials."
account=$(jq -r .Account <<<"$caller")
partition=$(jq -r .Arn <<<"$caller" | cut -d: -f2)
secret_arn=$(aws secretsmanager describe-secret --secret-id "$secret_name" --query ARN --output text) \
  || fail "Can't find the smoke API key secret '$secret_name'."

# Logs plus reading the key, nothing else.
log "role $name"
boundary=()
[ -n "${SMOKE_BOUNDARY_ARN:-}" ] && boundary=(--permissions-boundary "$SMOKE_BOUNDARY_ARN")
aws iam create-role --role-name "$name" ${boundary[@]+"${boundary[@]}"} --assume-role-policy-document \
  '{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"lambda.amazonaws.com"},"Action":"sts:AssumeRole"}]}' \
  >/dev/null || fail "Can't create role $name."
role_created=true
aws iam put-role-policy --role-name "$name" --policy-name smoke --policy-document "$(jq -nc \
  --arg logs "arn:$partition:logs:$region:$account:log-group:/aws/lambda/$name:*" --arg secret "$secret_arn" \
  '{Version: "2012-10-17", Statement: [
     {Effect: "Allow", Action: ["logs:CreateLogGroup", "logs:CreateLogStream", "logs:PutLogEvents"], Resource: [$logs]},
     {Effect: "Allow", Action: "secretsmanager:GetSecretValue", Resource: $secret}]}')" \
  || fail "Can't attach the role policy."

printf 'def handler(event, context):\n    print(event["marker"])\n    return "ok"\n' > "$tmp/app.py"
(cd "$tmp" && zip -q fn.zip app.py) || fail "Can't package the test function."

# A new role takes a few seconds before Lambda can assume it.
log "function $name with $layer_arn"
for _ in $(seq 1 12); do
  if aws lambda create-function --function-name "$name" --runtime python3.12 --architectures "$arch" \
      --handler app.handler --zip-file "fileb://$tmp/fn.zip" --timeout 30 --memory-size 256 \
      --role "arn:$partition:iam::$account:role/$name" --layers "$layer_arn" \
      --environment "Variables={CX_SECRET=$secret_arn,CX_DOMAIN=$SMOKE_DOMAIN,CX_APPLICATION=smoke,CX_SUBSYSTEM=$subsystem,CX_REPORTING_STRATEGY=LOW_OVERHEAD}" \
      >/dev/null 2>"$tmp/err"; then
    fn_created=true; break
  fi
  grep -q "cannot be assumed" "$tmp/err" || fail "create-function was refused: $(tail -n1 "$tmp/err")"
  sleep 10
done
$fn_created || fail "Lambda could not assume $name after two minutes."
aws lambda wait function-active-v2 --function-name "$name" || fail "Function $name never became active."

log "marker: $marker"
# A successful Invoke call can still carry a function error (an unhandled exception or a
# timeout), so each invocation must also return the handler's "ok".
for i in 1 2 3; do
  meta=$(aws lambda invoke --function-name "$name" --cli-binary-format raw-in-base64-out \
    --payload "$(jq -nc --arg m "$marker" '{marker: $m}')" --output json "$tmp/out.json") || fail "Invoke $i failed."
  function_error=$(jq -r '.FunctionError // empty' <<<"$meta")
  [ -z "$function_error" ] || fail "Invocation $i returned FunctionError=$function_error: $(head -c 300 "$tmp/out.json")"
  [ "$(cat "$tmp/out.json")" = '"ok"' ] || fail "Invocation $i returned $(head -c 300 "$tmp/out.json"), not \"ok\"."
  sleep 2
done

# Writes the response to $tmp/query and prints the HTTP status (000 when there was no
# response), so a rejected query is told apart from a marker that hasn't arrived yet.
query_marker() {
  local dataprime="source logs | filter \$l.subsystemname == '$subsystem' | limit 50"
  if [ -n "${SMOKE_QUERY_CMD:-}" ]; then
    if $SMOKE_QUERY_CMD "$dataprime" >"$tmp/query" 2>/dev/null; then echo 200; else echo 000; fi
  else
    local start end
    start=$(python3 -c "import datetime as d; print(d.datetime.fromtimestamp($start_ms/1000 - 300, d.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))")
    end=$(python3 -c "import datetime as d; print((d.datetime.now(d.timezone.utc) + d.timedelta(minutes=5)).strftime('%Y-%m-%dT%H:%M:%SZ'))")
    jq -n --arg q "$dataprime" --arg s "$start" --arg e "$end" \
      '{query: $q, metadata: {tier: "TIER_FREQUENT_SEARCH", syntax: "QUERY_SYNTAX_DATAPRIME", startDate: $s, endDate: $e, limit: 50}}' \
      | curl -sS --connect-timeout 10 --max-time 20 -o "$tmp/query" -w '%{http_code}' \
          -X POST "https://api.${SMOKE_DOMAIN}/api/v1/dataprime/query" \
          -H 'Content-Type: application/json' -H "Authorization: Bearer $SMOKE_QUERY_KEY" --data @- 2>/dev/null
  fi
}

fips=0 errors=0 logs_read=false delivered=false query_error=""
deadline=$(( $(date +%s) + 900 ))  # delivery took 3-6 minutes in testing
while [ "$(date +%s)" -lt "$deadline" ]; do
  sleep 15
  if out=$(aws logs filter-log-events --log-group-name "/aws/lambda/$name" --start-time "$start_ms" --output json 2>/dev/null); then
    logs_read=true
    fips=$(jq '[.events[].message | select(test("Starting coralogix-aws-lambda-telemetry-exporter .* FIPS"))] | length' <<<"$out")
    errors=$(jq '[.events[].message | select(test("ERROR|panicked|Status: (timeout|error)"))] | length' <<<"$out")
  fi
  [ "$errors" -eq 0 ] || break
  if ! $delivered; then
    status=$(query_marker)
    case "$status" in
      2??) grep -qF "$marker" "$tmp/query" && delivered=true ;;
      401|403) fail "Coralogix rejected the query (HTTP $status). Check that the query key is a DataQuerying key for the test team on $SMOKE_DOMAIN." ;;
      *) query_error="HTTP $status" ;;
    esac
  fi
  log "fips=$fips errors=$errors delivered=$delivered"
  [ "$fips" -gt 0 ] && $delivered && break
done

if [ "$errors" -gt 0 ]; then
  jq -r '.events[].message' <<<"$out" | grep -E "ERROR|panicked|Status: (timeout|error)" | head -20
  fail "The exporter logged errors."
fi
$logs_read || fail "Couldn't read /aws/lambda/$name."
[ "$fips" -gt 0 ] || fail "No FIPS startup line from the exporter."
$delivered || fail "The marker never reached Coralogix ($SMOKE_DOMAIN, subsystem $subsystem)${query_error:+; last query: $query_error}."

log "PASS: $layer_arn ($arch) started in FIPS mode and delivered the marker"
[ -n "${GITHUB_STEP_SUMMARY:-}" ] && echo "- $arch $layer_arn: PASS" >> "$GITHUB_STEP_SUMMARY"
result=0
