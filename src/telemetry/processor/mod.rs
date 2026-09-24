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

use lambda_extension::Status;
use serde::Serialize;
use std::collections::HashMap;

use super::PlatformReportData;

mod any_value;
mod epsagon_trace_processor;
mod log_maker;
mod log_processor;
mod platform_metrics;
mod span_processor;
pub mod telemetry_processor;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize)]
pub struct LogMetadata {
    // trace correlation
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    span_id: Option<String>,

    // analogous to OTEL attributes used for metrics and traces
    #[serde(skip_serializing_if = "Option::is_none")]
    cloud_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cloud_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cloud_region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    faas_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    faas_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    faas_instance_cx_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    faas_execution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    faas_invocation_id: Option<String>,

    #[serde(flatten)]
    extra_attributes: HashMap<String, String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "platform_event_type",
    content = "event",
    rename_all = "snake_case"
)]
pub enum PlatformEventLog {
    Start {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        version: Option<String>,
    },
    RuntimeDone {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        aws_status: String,
        aws_error_type: Option<String>,
        handler_status: Option<String>,
        handler_status_description: Option<String>,
        metrics: RuntimeDoneLogMetrics,
    },
    Report {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        status: String,
        error_type: Option<String>,
        out_of_memory: bool,
        metrics: ReportLogMetrics,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeDoneLogMetrics {
    duration_ms: Option<f64>,
    produced_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportLogMetrics {
    duration_ms: f64,
    billed_duration_ms: u64,
    memory_size_mb: u64,
    max_memory_used_mb: u64,
    init_duration_ms: Option<f64>,
    restore_duration_ms: Option<f64>,
    produced_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "platform_event_type",
    content = "event",
    rename_all = "snake_case"
)]
pub enum PlatformEventLogV2 {
    Start {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    RuntimeDone {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        aws_status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        aws_error_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        handler_status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        handler_status_description: Option<String>,
        metrics: RuntimeDoneLogMetricsV2,
    },
    Report {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error_type: Option<String>,
        #[serde(skip_serializing_if = "is_false")]
        out_of_memory: bool,
        metrics: ReportLogMetricsV2,
    },
}

fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeDoneLogMetricsV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    produced_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportLogMetricsV2 {
    duration_ms: f64,
    billed_duration_ms: u64,
    memory_size_mb: u64,
    max_memory_used_mb: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    init_duration_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    restore_duration_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    produced_bytes: Option<u64>,
}

fn span_id_to_string(span_id: &Vec<u8>) -> String {
    hex::encode(span_id)
}

fn trace_id_to_string(trace_id: &Vec<u8>) -> String {
    hex::encode(trace_id)
}

// This is a heuristic way of determining is function failed due to OOM. AWS doesn't reliably share that info with us.
fn is_out_of_memory(report_data: &PlatformReportData) -> bool {
    let explicitly_oomed = report_data.error_type.as_deref() == Some("Runtime.OutOfMemory");
    // On rare occasions, max_memory_used_mb will actually exceed memory_size_mb by 1MB
    let used_all_mem = report_data.metrics.max_memory_used_mb >= report_data.metrics.memory_size_mb;
    explicitly_oomed || (used_all_mem && report_data.status == Status::Error)
}

pub use platform_metrics::MAX_TIME_BETWEEN_DATA_NANOS;
pub use platform_metrics::PlatformMetricsState;
pub use platform_metrics::REPORT_LAST_TIME_NANOS;
