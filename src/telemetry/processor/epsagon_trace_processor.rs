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

use crate::Error;
use tracing::warn;

pub enum EpsagonTraceDetectionResult {
    RegularLog(String),
    EpsagonTraceJson(String),
}

const EPSAGON_TRACE_PREFIX: &str = "EPSAGON_TRACE: ";

pub fn detect_epsagon_trace(log_text: String) -> EpsagonTraceDetectionResult {
    if let Some(base64_string) = log_text.strip_prefix(EPSAGON_TRACE_PREFIX) {
        match decode_base64_to_string(base64_string.trim()) {
            Ok(trace_string) => EpsagonTraceDetectionResult::EpsagonTraceJson(trace_string),
            Err(error) => {
                warn!(
                    ?error,
                    "Failed to process base64 epsagon trace [{}]", base64_string
                );
                EpsagonTraceDetectionResult::RegularLog(log_text)
            }
        }
    } else {
        EpsagonTraceDetectionResult::RegularLog(log_text)
    }
}

fn decode_base64_to_string(base64_string: &str) -> Result<String, Error> {
    let trace_bytes = base64_simd::STANDARD.decode_to_vec(base64_string.trim())?;
    let trace_string = String::from_utf8(trace_bytes)?;
    Ok(trace_string)
}
