#!/usr/bin/env bash
# shellcheck disable=SC2016,SC2034,SC2329  # checks are eval'd strings
# Checks ci/govcloud/sign.sh and publish-layer.sh against a stub `aws` that keeps S3
# objects, signing jobs and layer versions in a directory. The stub Signer returns
# different bytes for every job, like the real one.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
mkdir -p "$t/bin" "$t/state"

cat > "$t/bin/aws" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
svc=$1 op=$2; shift 2; pos=()
while [ $# -gt 0 ]; do
  case $1 in
    --region) region=$2; shift 2 ;; --layer-name) layer=$2; shift 2 ;; --arn) arn=$2; shift 2 ;;
    --description) desc=$2; shift 2 ;; --zip-file) zip=${2#fileb://}; shift 2 ;;
    --key|--prefix) key=$2; shift 2 ;; --body) body=$2; shift 2 ;; --job-id) job=$2; shift 2 ;;
    --source) src=$2; shift 2 ;; --destination) dst=$2; shift 2 ;;
    --only-show-errors) shift ;; --*) shift 2 ;; *) pos+=("$1"); shift ;;
  esac
done
region=${region:-us-gov-west-1}
[ "${FAIL_REGION:-}" != "$region" ] || { echo "stub: $region is down" >&2; exit 255; }
echo "$svc $op $region" >> "$STATE/calls"
s3=$STATE/s3
case "$svc $op" in
  "s3api put-object")
    mkdir -p "$s3/$key.v"; v=$(( $(ls "$s3/$key.v" | wc -l) + 1 ))
    cp "$body" "$s3/$key.v/$v"; cp "$body" "$s3/$key"; echo "$v" ;;
  "s3api list-objects-v2")
    if [ -f "$s3/$key" ]; then echo "$key"; else echo None; fi ;;
  "s3 cp")
    from=${pos[0]} to=${pos[1]}
    if [ "$from" = - ]; then k=${to#s3://*/}; mkdir -p "$(dirname "$s3/$k")"; cat > "$s3/$k"
    else k=${from#s3://*/}; if [ "$to" = - ]; then cat "$s3/$k"; else cp "$s3/$k" "$to"; fi; fi ;;
  "signer start-signing-job")
    mkdir -p "$STATE/jobs"; j=job$(( $(ls "$STATE/jobs" | wc -l) + 1 ))
    k=$(sed 's/.*key=\([^,]*\),version=\([^}]*\)}/\1/' <<<"$src"); v=$(sed 's/.*version=\([^}]*\)}/\1/' <<<"$src")
    out=$(sed 's/.*prefix=\([^}]*\)}/\1/' <<<"$dst")$j.zip
    mkdir -p "$(dirname "$s3/$out")"; { cat "$s3/$k.v/$v"; echo "signature $j"; } > "$s3/$out"
    jq -n --arg k "$out" '{signedObject: {s3: {key: $k}}, profileVersion: "v1"}' > "$STATE/jobs/$j"
    echo "$j" ;;
  "signer wait") ;;
  "signer describe-signing-job") cat "$STATE/jobs/$job" ;;
  "signer get-signing-profile") echo '{"profileVersion":"v1","profileVersionArn":"arn:aws-us-gov:signer:us-gov-west-1:019051664735:/signing-profiles/p/v1"}' ;;
  "lambda list-layer-versions")
    d=$STATE/layers/$region/$layer; mkdir -p "$d"
    jq -s '{LayerVersions: .}' "$d"/*.json 2>/dev/null || echo '{"LayerVersions":[]}' ;;
  "lambda get-layer-version-by-arn")
    jq -r .Content.CodeSha256 "$(grep -l "\"$arn\"" "$STATE"/layers/*/*/*.json)" ;;
  "lambda publish-layer-version")
    d=$STATE/layers/$region/$layer; mkdir -p "$d"; n=$(( $(ls "$d" | wc -l) + 1 ))
    a=arn:aws-us-gov:lambda:$region:019051664735:layer:$layer:$n
    jq -n --arg a "$a" --arg d "$desc" --arg s "$(openssl dgst -sha256 -binary "$zip" | base64)" \
      '{LayerVersionArn: $a, Description: $d, Content: {CodeSha256: $s}}' | tee "$d/$n.json" ;;
  *) echo "stub: unexpected aws $svc $op" >&2; exit 2 ;;
esac
STUB
chmod +x "$t/bin/aws"
export PATH="$t/bin:$PATH" STATE="$t/state" \
  BUCKET=b PROFILE=p VERSION=1.0.0 COMMIT=abc1234def RUN_URL=https://example/runs/1

echo good > "$t/good.zip"; echo other > "$t/other.zip"
sign() { "$here/sign.sh" arm64 "$1" "$t/signed.zip" 2>/dev/null; }
pub() { "$here/publish-layer.sh" arm64 "$1" "$t/signed.zip" "v1.0.0 abc1234" 2>/dev/null; }
count() { grep -c "^$1" "$STATE/calls" || true; }
fail=0
check() { if eval "$2"; then echo "ok    $1"; else echo "FAIL  $1"; fail=1; fi; }

# Attempt 1: sign, publish west, east is down.
r=$(sign "$t/good.zip")
check "first run signs" '[ "$(jq -r .reused <<<"$r")" = false ] && [ "$(count "signer start-signing-job")" = 1 ]'
r=$(pub us-gov-west-1)
check "first run publishes" '[ "$(jq -r .reused <<<"$r")" = false ] && [ "$(count "lambda publish-layer-version us-gov-west-1")" = 1 ]'
check "a region that is down fails" '! FAIL_REGION=us-gov-east-1 pub us-gov-east-1 >/dev/null'

# Attempt 2 (re-run failed jobs: same build): reuses the signing, west and publishes east.
rm "$t/signed.zip"
r=$(sign "$t/good.zip")
check "retry reuses the signing job" '[ "$(jq -r .reused <<<"$r")" = true ] && [ "$(count "signer start-signing-job")" = 1 ]'
r=$(pub us-gov-west-1)
check "retry reuses the published version" '[ "$(jq -r .reused <<<"$r")" = true ] && [[ $(jq -r .arn <<<"$r") == *:1 ]]'
pub us-gov-east-1 >/dev/null
check "retry publishes only the missing region" '[ "$(count "lambda publish-layer-version us-gov-west-1")" = 1 ] && [ "$(count "lambda publish-layer-version us-gov-east-1")" = 1 ]'

# A fresh build under the same release.
check "a different build is not signed again" '! sign "$t/other.zip" >/dev/null && [ "$(count "signer start-signing-job")" = 1 ]'
echo tampered > "$t/signed.zip"
check "a different signed ZIP is not published" '! pub us-gov-west-1 >/dev/null && [ "$(count "lambda publish-layer-version us-gov-west-1")" = 1 ]'

# A test rehearsal of the same commit: its own signing records and -test layer names.
r=$(export TARGET=test; sign "$t/other.zip")
check "a test run signs separately from production" '[ "$(jq -r .reused <<<"$r")" = false ] && [ -f "$STATE/s3/signed/test/1.0.0/abc1234def/arm64.json" ]'
r=$(export NAME_SUFFIX=-test; pub us-gov-west-1)
check "a test run publishes the -test layer" '[[ $(jq -r .arn <<<"$r") == *:layer:coralogix-aws-lambda-telemetry-exporter-arm64-test:1 ]]'

exit $fail
