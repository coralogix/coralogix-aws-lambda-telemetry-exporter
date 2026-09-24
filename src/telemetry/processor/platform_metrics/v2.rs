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

use super::instruments::{Counter, Descriptor, MaxGauge, Summary};
use crate::proto::opentelemetry::proto::metrics::v1::Metric;
use crate::telemetry::processor::is_out_of_memory;
use crate::telemetry::{
    PlatformInitReportData, PlatformReportData, PlatformRuntimeDoneData, now_nanos,
};
use lambda_extension::Status;
use tracing::trace;

#[derive(Debug)]
pub struct PlatformMetricsState {
    pub start_timestamp_nanos: u64,
    pub latest_data_timestamp_nanos: u64,
    pub metrics: PlatformMetrics,
}

#[allow(clippy::new_without_default)]
impl PlatformMetricsState {
    pub fn new() -> PlatformMetricsState {
        let now = now_nanos();
        PlatformMetricsState {
            start_timestamp_nanos: now,
            latest_data_timestamp_nanos: now,
            metrics: PlatformMetrics::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlatformMetrics {
    // init metrics
    pub init_duration_seconds: Summary,

    // runtime done metrics
    pub errors: Counter,
    pub timeouts: Counter,
    pub invoke_duration_seconds: Summary,

    // Report metrics
    pub probable_ooms: Counter,
    pub billed_duration_seconds: Summary,
    pub memory_used_bytes: MaxGauge,
    pub memory_limit_bytes: MaxGauge,
}

impl Default for PlatformMetrics {
    fn default() -> Self {
        PlatformMetrics {
            // Using prometheus' "seconds" and "bytes" in name instead of OTEL (UCUM) "s", "By" in unit
            // One reason for this is that the only way CX users interact with units is via PromQL, so keeping Promethus' conventions offers better UX
            // Another is that metrics-gateway currently sometimes omits the unit (NGSTN-1087)

            // https://github.com/open-telemetry/semantic-conventions/blob/v1.27.0/docs/faas/faas-metrics.md
            // According to OTEL semantic convention this is supposed to be a histogram, but that's too expensive without server-side aggregation.
            init_duration_seconds: Summary::new(Descriptor {
                name: "faas.init_duration_seconds",
                description: "Duration of Lambda execution environment initialization",
                unit: "",
            }),

            // https://github.com/open-telemetry/semantic-conventions/blob/v1.27.0/docs/faas/faas-metrics.md
            errors: Counter::new(Descriptor {
                name: "faas.errors",
                description: "Number of errors",
                unit: "",
            }),
            // https://github.com/open-telemetry/semantic-conventions/blob/v1.27.0/docs/faas/faas-metrics.md
            timeouts: Counter::new(Descriptor {
                name: "faas.timeouts",
                description: "Number of timeouts",
                unit: "",
            }),
            // https://github.com/open-telemetry/semantic-conventions/blob/v1.27.0/docs/faas/faas-metrics.md
            // According to OTEL semantic convention this is supposed to be a histogram, but that's too expensive without server-side aggregation.
            invoke_duration_seconds: Summary::new(Descriptor {
                name: "faas.invoke_duration_seconds",
                description: "Duration of Lambda function code execution",
                unit: "",
            }),

            // This is out of OTEL spec
            probable_ooms: Counter::new(Descriptor {
                name: "faas.probable_ooms",
                description: "Number of Errors, that are likely to have been caused by running out of memory",
                unit: "",
            }),
            // This metric is not defined by OTEL
            billed_duration_seconds: Summary::new(Descriptor {
                name: "faas.billed_duration_seconds",
                description: "Duration of Lambda function execution that is billed",
                unit: "",
            }),
            // This metric is already calculated as a maximum by AWS.
            // So it makes sense for us to further aggregate max_memory_used_mb from multiple invocations by taking a maximum (as opposed to for example averaging them).
            // https://github.com/open-telemetry/semantic-conventions/blob/v1.27.0/docs/faas/faas-metrics.md
            memory_used_bytes: MaxGauge::new(Descriptor {
                name: "faas.mem_usage_bytes",
                description: "Largest amount of memory used by the lambda function (and lambda extensions)",
                unit: "",
            }),
            // This metric won't change over time for given instance
            // This metric is not defined in OTEL semantic conventions, but it's useful as a reference for faas.mem_usage.
            // The OTEL semantic conventions recommend adding the mem limit as an attribute to the resource, but that's not so useful.
            // Using "limit" in the name as that's recommended by OTEL docs: https://opentelemetry.io/docs/reference/specification/metrics/semantic_conventions/#instrument-naming
            memory_limit_bytes: MaxGauge::new(Descriptor {
                name: "faas.mem_limit_bytes",
                description: "Size of memory configured for the lambda function",
                unit: "",
            }),
        }
    }
}

pub fn update_metrics_with_init_report(
    metrics_state: &mut PlatformMetricsState,
    init_report: &PlatformInitReportData,
) {
    let metrics = &mut metrics_state.metrics;
    metrics
        .init_duration_seconds
        .record(init_report.metrics.duration_ms / 1000.0);
}

pub fn update_metrics_with_runtime_done(
    metrics_state: &mut PlatformMetricsState,
    runtime_done: &PlatformRuntimeDoneData,
) {
    let metrics = &mut metrics_state.metrics;

    if let Some(runtime_duration_ms) = runtime_done.metrics.as_ref().map(|x| x.duration_ms) {
        metrics
            .invoke_duration_seconds
            .record(runtime_duration_ms / 1000.0);
    }

    match runtime_done.status {
        Status::Success => (),
        Status::Error | Status::Failure => metrics.errors.record(1),
        Status::Timeout => metrics.timeouts.record(1),
    }

    metrics_state.latest_data_timestamp_nanos = runtime_done.time.unix_timestamp_nanos() as u64;
}

pub fn update_metrics_with_report(
    metrics_state: &mut PlatformMetricsState,
    report: &PlatformReportData,
) {
    let metrics = &mut metrics_state.metrics;

    metrics
        .memory_used_bytes
        .record(report.metrics.max_memory_used_mb as i64 * 1024 * 1024);

    metrics
        .memory_limit_bytes
        .record(report.metrics.memory_size_mb as i64 * 1024 * 1024); // this won't change during the lifetime of the extension, but we want to keep reporting it

    metrics
        .billed_duration_seconds
        .record(report.metrics.billed_duration_ms as f64 / 1000.0);

    if report.status == Status::Error && is_out_of_memory(report) {
        metrics.probable_ooms.record(1)
    }
}

pub fn make_metrics_report(state: &mut PlatformMetricsState) -> Vec<Metric> {
    let start_timestamp_nanos = state.start_timestamp_nanos;
    let timestamp_nanos = state.latest_data_timestamp_nanos;

    let metrics = &mut state.metrics;

    let counters: Vec<Metric> = vec![
        &mut metrics.errors,
        &mut metrics.probable_ooms,
        &mut metrics.timeouts,
    ]
    .into_iter()
    .filter_map(|i| i.report(start_timestamp_nanos, timestamp_nanos))
    .collect();

    let summaries: Vec<Metric> = vec![
        &mut metrics.init_duration_seconds,
        &mut metrics.invoke_duration_seconds,
        &mut metrics.billed_duration_seconds,
    ]
    .into_iter()
    .filter_map(|i| i.report(start_timestamp_nanos, timestamp_nanos))
    .collect();

    let maxes: Vec<Metric> = vec![
        &mut metrics.memory_used_bytes,
        &mut metrics.memory_limit_bytes,
    ]
    .into_iter()
    .filter_map(|i| i.report(start_timestamp_nanos, timestamp_nanos))
    .collect();

    let report: Vec<Metric> = counters.into_iter().chain(summaries).chain(maxes).collect();

    trace!("Made metrics report containing {} metrics", report.len());

    report
}
