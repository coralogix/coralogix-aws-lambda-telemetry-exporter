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

use std::env;

use ErrorHandlingMode::*;
use tracing::error;
use tracing::warn;

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum ErrorHandlingMode {
    DontCrash,
    Crash,
}

impl ErrorHandlingMode {
    pub fn handle_error(&self, message: &str) {
        match self {
            DontCrash => error!("{message}"),
            Crash => {
                error!("{message}");
                // because of `panic = "abort"` configured in `Cargo.toml` this will crash the process and AWS will shut down the lambda function instance
                panic!("coralogix-aws-lambda-telemetry-exporter is crashing due to fatal error");
            }
        }
    }

    pub fn from_env() -> ErrorHandlingMode {
        env::var("CX_ERROR_HANDLING").map_or(ErrorHandlingMode::DontCrash, |s| {
            match s.trim().to_lowercase().as_str() {
                "dont_crash" => DontCrash,
                "crash" => Crash,
                other => {
                    warn!("Invalid value of CX_ERROR_HANDLING: {other}");
                    DontCrash
                }
            }
        })
    }
}
