
# Working on Coralogix AWS Lambda Telemetry Exporter

## Local setup (macOS)

* install [rustup](https://rustup.rs/)
* install [cargo-lambda](https://www.cargo-lambda.info/guide/installation.html)
* install cross-compilation toolchains:

    ```sh
    brew tap messense/macos-cross-toolchains
    brew install messense/macos-cross-toolchains/aarch64-unknown-linux-gnu messense/macos-cross-toolchains/x86_64-unknown-linux-gnu
    rustup target add aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu
    ```

* configure an AWS CLI profile

## Building

Verify changes with: `cargo fmt --all && cargo clippy --all-targets && cargo test`

## Deployment and manual testing in AWS

1. Run `./scripts/package_and_publish_development.sh <AWS profile> <AWS region>`
2. Log in to AWS management console
3. Go to `Lambda` -> `Functions` -> open a test function. Find the `Layers` section on the bottom. Either add or update the `coralogix-aws-lambda-telemetry-exporter-*-development` layer.
4. Go to the `Test` tab in the view of the lambda function, and click test (no need to do anything with the test event)
5. Click the `logs` link to go to CloudWatch logs
6. You can go to coralogix test account to verify that the logs, traces and metrics have been delivered and are correct.

## Publishing a release version

1. Bump and commit the version in `Cargo.toml` on a branch
2. Open a PR and merge it to `master` / `release` branch
3. Go to github actions and run the `publish` workflow. Choose branch `master` and provide the same version number that you set in `Cargo.toml`
4. Verify the result of the workflow including the ARNs of published layers
5. Update documentation
