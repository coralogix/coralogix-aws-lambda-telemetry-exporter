// Copyright 2026 Coralogix Ltd.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//         http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::path::PathBuf;

use protofetch::{LockMode, Protofetch};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=protofetch.toml");
    println!("cargo:rerun-if-changed=protofetch.lock");
    println!("cargo:rerun-if-changed=protofetch/config.toml");

    if std::env::var_os("PROTOFETCH_CONFIG_DIR").is_none() {
        let config_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?).join("protofetch");
        // SAFETY: build scripts run in a single-threaded process before protofetch runs.
        unsafe { std::env::set_var("PROTOFETCH_CONFIG_DIR", config_dir) };
    }

    Protofetch::builder()
        .try_build()?
        .fetch(if is_ci::cached() {
            LockMode::Locked
        } else {
            LockMode::Update
        })?;

    let mut config = prost_build::Config::new();
    config.disable_comments(["."]);

    tonic_prost_build::configure()
        .compile_well_known_types(false)
        .build_server(true)
        .build_client(true)
        .compile_with_config(
            config,
            &[
                "target/proto/opentelemetry/proto/collector/logs/v1/logs_service.proto",
                "target/proto/opentelemetry/proto/collector/trace/v1/trace_service.proto",
                "target/proto/opentelemetry/proto/collector/metrics/v1/metrics_service.proto",
                "target/proto/com/coralogix/ingress/otlp/v1/telemetry_service.proto",
            ],
            &["target/proto"],
        )?;

    built::write_built_file().expect("Failed to acquire build-time information");

    Ok(())
}
