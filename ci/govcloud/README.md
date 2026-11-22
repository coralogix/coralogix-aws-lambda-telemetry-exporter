# GovCloud build

The GovCloud layers use the same `Cargo.lock` as commercial, built with the `fips`
feature. That links the AWS-LC FIPS v4 module, validated by NIST as certificate #5596
(AWS-LC FIPS 4.0.0); later 4.x releases are accepted as its update stream under
FedRAMP.

`build.sh` builds natively on Amazon Linux 2023, so the layer supports the AL2023
runtimes only. It runs clippy and the tests, then packages the layer and checks its
architecture, glibc (2.34 or lower) and that exactly one `AWS-LC FIPS 4.x` module is
linked. CI also runs `cargo deny` on the FIPS build with `deny.toml`.

```bash
docker build -t govcloud-builder -f ci/govcloud/Dockerfile.builder ci/govcloud
docker run --rm -v "$PWD":/work -w /work govcloud-builder ci/govcloud/build.sh
```
