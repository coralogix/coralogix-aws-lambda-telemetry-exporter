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

`.github/workflows/publish-govcloud.yml` is the GovCloud counterpart of `publish.yml`, run
with a version and an environment:

- `govcloud-test`: any branch; publishes `-test` layers, which stay private.
- `govcloud`: `master` or `release*` branches; the release.

Like commercial it checks the layer versions are in sync across regions, signs both
packages with AWS Signer and publishes them with `scripts/publish_<environment>.sh`
(regions, name suffix and the AL2023 runtimes are in `scripts/set_<environment>_vars.sh`).
It differs only where GovCloud requires it: the FIPS build on AL2023, OIDC instead of
stored keys, and publishing private, then smoke-testing, then making the version public
(`govcloud` only).

If a publish fails part-way, the regions end up on different versions and the next run
stops at the regional sync check. Run `sync-region-govcloud.yml` (the counterpart of
`sync-region.yml`) for `us-gov-east-1` to copy the missing versions from
`us-gov-west-1`, then publish again.

The environments need `GOVCLOUD_ROLE_ARN`, `GOVCLOUD_SIGNER_PROFILE`,
`GOVCLOUD_SIGNING_BUCKET_PREFIX` (the bucket is `<prefix>-us-gov-west-1`) and the smoke
settings below. The role must trust both environments' OIDC subjects and needs, in the
signing bucket, `s3:PutObject`, `s3:GetObject` and `s3:ListBucketVersions` on
`to-be-signed/*` and `signed/*`; `signer:StartSigningJob` and `DescribeSigningJob`; and
`lambda:ListLayerVersions`, `GetLayerVersion`, `PublishLayerVersion`,
`AddLayerVersionPermission` and `GetLayerVersionPolicy` in each region.

## Smoke test

`smoke.sh` attaches a private layer version to a temporary `python3.12` function in
us-gov-west-1, invokes it with a unique marker and passes only when the exporter logs its
FIPS startup line, logs no errors and the marker reaches the Coralogix test team. Every
`smoke-exporter-*` resource is removed afterwards.

Each environment needs `GOVCLOUD_SMOKE_DOMAIN`, `GOVCLOUD_SMOKE_BOUNDARY_ARN` (optional)
and the `GOV_SMOKE_QUERY_KEY` secret (a DataQuerying key); the account needs the Secrets
Manager secret `coralogix-gov-smoke-test-api-key` (the test team's Send-Your-Data key).
The role additionally needs `iam:CreateRole`,
`PutRolePolicy`, `DeleteRolePolicy`, `DeleteRole` and `PassRole` on `role/smoke-exporter-*`;
`lambda:CreateFunction`, `GetFunction`, `InvokeFunction`, `DeleteFunction` on
`function:smoke-exporter-*`; `logs:FilterLogEvents` and `DeleteLogGroup` on their log
groups; and `secretsmanager:DescribeSecret` on the key secret.

Run it locally against a layer version:

```bash
SMOKE_DOMAIN=eu2.coralogix.com SMOKE_QUERY_CMD=<query command> \
  ci/govcloud/smoke.sh <layer-version-arn> <region> arm64
```
