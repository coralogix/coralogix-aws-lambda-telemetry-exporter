# GovCloud build

The GovCloud layers are built with the `fips` feature, which links the AWS-LC FIPS v4
module, validated by NIST as certificate #5596 (AWS-LC FIPS 4.0.0); later 4.x releases
are accepted as its update stream under FedRAMP.

`build.sh` builds natively on Amazon Linux 2023, so the layer supports the AL2023
runtimes only. It runs clippy and the tests, then packages the layer and checks its
architecture, glibc (2.34 or lower) and that exactly one `AWS-LC FIPS 4.x` module is
linked. CI also runs `cargo deny` on the FIPS build with `deny.toml`.

```bash
docker build -t govcloud-builder -f ci/govcloud/Dockerfile.builder ci/govcloud
docker run --rm -v "$PWD":/work -w /work govcloud-builder ci/govcloud/build.sh
```

## Publishing

`.github/workflows/publish-govcloud.yml` (run from `master` with the version in
`Cargo.toml`) checks advisories, builds, signs and publishes **private** layer versions,
then smoke-tests them; it never makes one public.

If it fails part-way, use **Re-run failed jobs** on the same run. That keeps the build
artifacts (kept 90 days) and reuses the signing recorded at
`s3://<signing bucket>/signed/<version>/<commit>/<arch>.json`, so already published regions
are reused and only missing ones are published. A new run for the same version and commit
builds new bytes and fails at signing; it names the run to re-run instead.

The `govcloud` environment role needs, in the signing bucket: `s3:PutObject` and
`s3:GetObject` (and `s3:GetObjectVersion`) on `to-be-signed/*` and `signed/*`, and
`s3:ListBucket` (to look up the signing record); `signer:StartSigningJob`,
`DescribeSigningJob`, `GetSigningProfile`; and `lambda:ListLayerVersions`,
`GetLayerVersion`, `PublishLayerVersion` in each region.

## Smoke test

`smoke.sh` attaches a private layer version to a temporary `python3.12` function in
us-gov-west-1, invokes it with a unique marker and passes only when the exporter logs its
FIPS startup line, logs no errors and the marker reaches the Coralogix test team. Every
`smoke-exporter-*` resource is removed afterwards.

The job is skipped until the `GOVCLOUD_SMOKE_DOMAIN` repository variable is set. It also
uses `GOVCLOUD_SMOKE_BOUNDARY_ARN` (optional), the `GOV_SMOKE_QUERY_KEY` secret (a
DataQuerying key) and the Secrets Manager secret `coralogix-gov-smoke-test-api-key` (the
test team's Send-Your-Data key). The role additionally needs `iam:CreateRole`,
`PutRolePolicy`, `DeleteRolePolicy`, `DeleteRole` and `PassRole` on `role/smoke-exporter-*`;
`lambda:CreateFunction`, `GetFunction`, `InvokeFunction`, `DeleteFunction` on
`function:smoke-exporter-*`; `logs:FilterLogEvents` and `DeleteLogGroup` on their log
groups; and `secretsmanager:DescribeSecret` on the key secret.

Run it locally against a layer version:

```bash
SMOKE_DOMAIN=eu2.coralogix.com SMOKE_QUERY_CMD=<query command> \
  ci/govcloud/smoke.sh <layer-version-arn> <region> arm64
```
