# GovCloud FIPS builder: Amazon Linux 2023, the same glibc (2.34) as the AL2023 Lambda
# runtimes the GovCloud layer supports. aws-lc-fips-sys is built with the native
# compiler; the Rust toolchain is installed at run time by ci/govcloud/build.sh so it
# always follows rust-toolchain.toml.
FROM amazonlinux:2023

# hadolint ignore=DL3041
RUN dnf install -y --setopt=retries=25 --setopt=timeout=60 \
        gcc gcc-c++ make cmake golang perl perl-FindBin perl-IPC-Cmd perl-Digest-SHA \
        clang protobuf-compiler openssl-devel python3 tar gzip zip unzip git findutils file binutils \
    && dnf clean all

ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
