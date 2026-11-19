#!/usr/bin/env bash
# Sign one GovCloud layer ZIP with AWS Signer and download the signed ZIP, or reuse the
# signing a previous attempt of this release already did.
#
#   BUCKET=... PROFILE=... VERSION=... COMMIT=... RUN_URL=... \
#     ci/govcloud/sign.sh <arch> <zip> <signed-zip-out>
#
# Signer output differs on every job, and publish-layer.sh rejects a second set of bytes
# under the same release. So each successful signing is recorded at
# s3://$BUCKET/signed/$VERSION/$COMMIT/$arch.json, and a retry reuses it:
#   - same unsigned ZIP -> download the recorded signed ZIP (hash checked), no new job;
#   - different unsigned ZIP (a fresh build) -> fail; re-run the failed jobs of the
#     recorded run instead, which keeps its build artifacts.
#
# Uses the AWS region from the environment (the signing bucket's region). Prints one JSON
# line: {"arch","unsigned_sha256","source_key","source_version","job_id",
# "profile_version_arn","signed_key","signed_sha256","run","reused"}.
set -euo pipefail

arch=$1 zip=$2 out=$3
: "${BUCKET:?}" "${PROFILE:?}" "${VERSION:?}" "${COMMIT:?}" "${RUN_URL:?}"

unsigned=$(sha256sum "$zip" | cut -d' ' -f1)
key=to-be-signed/$VERSION/$COMMIT/package-$arch-fips.zip
record_key=signed/$VERSION/$COMMIT/$arch.json

# Needs s3:ListBucket; without it a missing record can't be told from a denied one.
found=$(aws s3api list-objects-v2 --bucket "$BUCKET" --prefix "$record_key" \
  --query 'Contents[0].Key' --output text) \
  || { echo "::error::cannot list s3://$BUCKET/$record_key; the role needs s3:ListBucket on the signing bucket" >&2; exit 1; }
if [ "$found" = "$record_key" ]; then
  record=$(aws s3 cp --only-show-errors "s3://$BUCKET/$record_key" -)
  [ "$(jq -r .unsigned_sha256 <<<"$record")" = "$unsigned" ] || {
    echo "::error::v$VERSION at $COMMIT ($arch) was already signed from another build ($(jq -r .unsigned_sha256 <<<"$record")) in $(jq -r .run <<<"$record"). Re-run that run's failed jobs instead." >&2
    exit 1
  }
  aws s3 cp --only-show-errors "s3://$BUCKET/$(jq -r .signed_key <<<"$record")" "$out"
  [ "$(sha256sum "$out" | cut -d' ' -f1)" = "$(jq -r .signed_sha256 <<<"$record")" ] \
    || { echo "::error::the signed ZIP no longer matches $record_key" >&2; exit 1; }
  echo "$arch: reusing signing job $(jq -r .job_id <<<"$record")" >&2
  jq -c '.reused = true' <<<"$record"
  exit 0
fi

# The bucket is versioned; the version ID pins Signer's input to these bytes.
source_version=$(aws s3api put-object --bucket "$BUCKET" --key "$key" --body "$zip" \
  --checksum-algorithm SHA256 --query VersionId --output text)
[ -n "$source_version" ] && [ "$source_version" != None ] \
  || { echo "::error::s3://$BUCKET/$key has no version ID; is the bucket versioned?" >&2; exit 1; }

job_id=$(aws signer start-signing-job --profile-name "$PROFILE" \
  --source "s3={bucketName=$BUCKET,key=$key,version=$source_version}" \
  --destination "s3={bucketName=$BUCKET,prefix=signed/$VERSION/$COMMIT/$arch-}" \
  --query jobId --output text)
echo "$arch: signing job $job_id" >&2
aws signer wait successful-signing-job --job-id "$job_id" \
  || { aws signer describe-signing-job --job-id "$job_id" --query '[status,statusReason]' --output text >&2; exit 1; }

job=$(aws signer describe-signing-job --job-id "$job_id" --output json)
signed_key=$(jq -r .signedObject.s3.key <<<"$job")
job_profile_version=$(jq -r .profileVersion <<<"$job")
profile=$(aws signer get-signing-profile --profile-name "$PROFILE" --output json)
# The ARN is only available for the profile's current version; make sure it is the one used.
[ "$(jq -r .profileVersion <<<"$profile")" = "$job_profile_version" ] \
  || { echo "::error::signing profile changed during the run (job used $job_profile_version)" >&2; exit 1; }
profile_version_arn=$(jq -r .profileVersionArn <<<"$profile")

aws s3 cp --only-show-errors "s3://$BUCKET/$signed_key" "$out"
signed=$(sha256sum "$out" | cut -d' ' -f1)
[ "$signed" != "$unsigned" ] || { echo "::error::the signed ZIP is identical to the unsigned one" >&2; exit 1; }

record=$(jq -nc --arg arch "$arch" --arg u "$unsigned" --arg k "$key" --arg v "$source_version" \
  --arg j "$job_id" --arg p "$profile_version_arn" --arg sk "$signed_key" --arg s "$signed" \
  --arg r "$RUN_URL" \
  '{arch: $arch, unsigned_sha256: $u, source_key: $k, source_version: $v, job_id: $j,
    profile_version_arn: $p, signed_key: $sk, signed_sha256: $s, run: $r}')
aws s3 cp --only-show-errors - "s3://$BUCKET/$record_key" <<<"$record"
jq -c '.reused = false' <<<"$record"
