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

`.github/workflows/publish-govcloud.yml` is the GovCloud counterpart of `publish.yml`. It
runs in the `govcloud` GitHub environment, from `master` only (`release*` branches are
not protected in this repo).

Like commercial it checks the layer versions are in sync across regions, signs both
packages with AWS Signer and publishes them with `scripts/publish_govcloud.sh` (regions
and the AL2023 runtimes are in `scripts/set_govcloud_vars.sh`). It differs only where
GovCloud requires it: the FIPS build on AL2023, OIDC instead of stored keys, and
publishing private, then smoke-testing, then making the version public. A failed run
never reaches customers; at worst it leaves an unused private version. There is no test
environment, since it would let any branch assume a GovCloud role.

If a publish fails part-way, the regions end up on different versions and the next run
stops at the regional sync check. Run `sync-region-govcloud.yml` (the counterpart of
`sync-region.yml`) for `us-gov-east-1` to copy the missing versions from
`us-gov-west-1`, then publish again. If only one architecture was published in
us-gov-west-1 (its x86_64 and arm64 versions differ), first publish the missing one there
from the failed run's `package-<arch>.zip` artifact (kept for a day) with
`aws lambda publish-layer-version`, using the same runtimes and architecture as
`scripts/publish_layers.sh`, then sync.

### Environment settings

The `govcloud` environment sets `AWS_ROLE_ARN`, `SIGNER_BUCKET` (in us-gov-west-1),
`SIGNER_PROFILE_NAME`, `SMOKE_DOMAIN` (the Coralogix test team's domain) and the
`SMOKE_QUERY_KEY` secret (a DataQuerying key for that team).

### AWS roles

The publishing role trusts only
`repo:coralogix/coralogix-aws-lambda-telemetry-exporter:environment:govcloud` and needs:

- `s3:PutObject`, `GetObject` and `GetObjectVersion` on `to-be-signed/govcloud/*` and
  `signed/govcloud/*` in the signing bucket (signing reads a specific source version);
  `s3:ListBucketVersions` on the bucket, limited to those prefixes with `s3:prefix`.
- `signer:StartSigningJob` and `DescribeSigningJob`.
- `lambda:ListLayerVersions`, `GetLayerVersion`, `PublishLayerVersion`,
  `GetLayerVersionPolicy` and `AddLayerVersionPermission` on
  `coralogix-aws-lambda-telemetry-exporter-x86_64` and `-arm64`, and the combined layers
  `coralogix-<python|nodejs|java>-wrapper-and-exporter-<x86_64|arm64>`, in both regions.
- For the smoke test: `lambda:CreateFunction`, `GetFunction`, `InvokeFunction` and
  `DeleteFunction` on `function:smoke-exporter-*`; `logs:FilterLogEvents` and
  `DeleteLogGroup` on `/aws/lambda/smoke-exporter-*`; `secretsmanager:DescribeSecret` on
  the smoke key secret; and `iam:PassRole` on `role/smoke-exporter` only.

`smoke-exporter` is a static role the smoke functions run as, created once: trusted by
`lambda.amazonaws.com`, allowed only `logs:CreateLogGroup`, `CreateLogStream` and
`PutLogEvents` on `/aws/lambda/smoke-exporter-*` and `secretsmanager:GetSecretValue` on
the smoke key secret. CI never creates or changes roles.

## Combined layers

`.github/workflows/publish-combined-govcloud.yml` is the GovCloud counterpart of
`publish-combined.yml`: it publishes `coralogix-<runtime>-wrapper-and-exporter-<arch>` for the
runtimes you pick, from `master` only, public on publish and without a smoke test, like
commercial. Each layer merges:

- the newest exporter version that is public for both architectures in both regions
  (`public-exporter-version.sh`), so it has passed the smoke test above. A newer private
  version from a failed run is never used;
- the latest commercial `coralogix-opentelemetry-<runtime>-wrapper` layer, unchanged. A
  GovCloud role can't read commercial layers, so the wrapper is fetched in the `production`
  environment.

The wrappers export only to the exporter on the same host (`http://localhost:4318`); only
the FIPS exporter connects to Coralogix. Keep the `OTEL_EXPORTER_OTLP_*` endpoints at their
defaults in GovCloud. The layers support AL2023 runtimes only (the `*_COMBINED_RUNTIMES` in
`scripts/set_govcloud_vars.sh`).

If a publish fails part-way, don't re-run it: run `sync-region-govcloud.yml`, which copies
missing combined versions public and makes the newest version public in both regions
(`ensure-public.sh`), including one whose grant failed. Then publish again. If only one
architecture of a runtime was published in us-gov-west-1 (its x86_64 and arm64 versions
differ), first publish the missing one there from the failed run's signed
`coralogix-<runtime>-wrapper-and-exporter-<arch>.zip` artifact (kept for a day) with
`aws lambda publish-layer-version`, using the runtimes in `scripts/set_govcloud_vars.sh`,
make it public with `ensure-public.sh`, then sync.

## Container images

`.github/workflows/publish-govcloud-images.yml` builds images for Lambdas deployed as
container images, which can't use layers. It uses the same `docker/Dockerfile` as
commercial (the layer contents in `/opt`), built from the newest layers already public in
GovCloud, so each image holds exactly the signed layer bytes. It pushes to the commercial
DockerHub repositories with a `-fips` tag, like the FIPS Coralogix Operator:

- `coralogixrepo/coralogix-aws-lambda-telemetry-exporter:<version>-fips`
- `coralogixrepo/coralogix-<runtime>-wrapper-and-exporter:gov-<layer version>-fips`

Both also get `latest-fips`. Run it after `publish-govcloud.yml` and
`publish-combined-govcloud.yml`, with the exporter version from the GovCloud layer. The
DockerHub credentials stay in the `production` environment.

Customers build `FROM` the image and push their function image to their own GovCloud ECR.
Build it with `--provenance=false`: Lambda rejects images with build attestations.

## Smoke test

`smoke.sh` attaches a private layer version to a temporary `python3.12` function, invokes
it with a unique marker and passes only when the exporter logs its FIPS startup line,
logs no errors and the marker reaches the Coralogix test team. The function and its log
group are removed afterwards. The account needs the Secrets Manager secret
`coralogix-gov-smoke-test-api-key` (the test team's Send-Your-Data key).

Run it locally against a layer version:

```bash
SMOKE_DOMAIN=eu2.coralogix.com SMOKE_QUERY_CMD=<query command> \
  ci/govcloud/smoke.sh <layer-version-arn> arm64
```
