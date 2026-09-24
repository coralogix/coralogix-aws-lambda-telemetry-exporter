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
use crate::telemetry::{ProcessingState, now_nanos};
use lambda_extension::{ReportMetrics, RuntimeDoneMetrics, Status};
use time::OffsetDateTime;
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
    pub coldstarts: Counter,
    pub executions: Counter,
    pub errors: Counter,
    pub ooms: Counter,
    pub timeouts: Counter,

    pub init_duration_millis: Summary,
    pub restore_duration_millis: Summary,
    pub invoke_duration_millis: Summary,
    pub overhead_duration_millis: Summary,

    pub result_size_bytes: Summary,

    pub max_memory_used_mb: MaxGauge,
    pub memory_size_mb: MaxGauge,
}

impl Default for PlatformMetrics {
    fn default() -> Self {
        PlatformMetrics {
            // UCUM units used as specified in https://github.com/open-telemetry/semantic-conventions/blob/v1.32.0/docs/general/metrics.md?plain=1#L98

            // https://github.com/open-telemetry/semantic-conventions/blob/6814d83dac7ebd6b29dada91c669a99275f674c8/docs/faas/faas-metrics.md?plain=1#L122
            coldstarts: Counter::new(Descriptor {
                name: "faas.coldstarts",
                description: "Number of coldstarts",
                unit: "",
            }),
            // https://github.com/open-telemetry/opentelemetry-specification/blob/v1.17.0/specification/metrics/semantic_conventions/faas-metrics.md?plain=1#L45
            // This has changed since and our implementation is out-of-date with the latest conventions https://github.com/open-telemetry/semantic-conventions/blob/6814d83dac7ebd6b29dada91c669a99275f674c8/docs/faas/faas-metrics.md?plain=1#L194
            executions: Counter::new(Descriptor {
                name: "faas.executions",
                description: "Number of executions",
                unit: "",
            }),
            // https://github.com/open-telemetry/semantic-conventions/blob/6814d83dac7ebd6b29dada91c669a99275f674c8/docs/faas/faas-metrics.md?plain=1#L158
            errors: Counter::new(Descriptor {
                name: "faas.errors",
                description: "Number of errors",
                unit: "",
            }),
            // This is out of OTEL spec
            ooms: Counter::new(Descriptor {
                name: "faas.ooms",
                description: "Number of Out of Memory errors",
                unit: "",
            }),
            // https://github.com/open-telemetry/semantic-conventions/blob/6814d83dac7ebd6b29dada91c669a99275f674c8/docs/faas/faas-metrics.md?plain=1#L230
            timeouts: Counter::new(Descriptor {
                name: "faas.timeouts",
                description: "Number of timeouts",
                unit: "",
            }),
            // https://github.com/open-telemetry/semantic-conventions/blob/6814d83dac7ebd6b29dada91c669a99275f674c8/docs/faas/faas-metrics.md?plain=1#L82
            // According to OTEL semantic convention this is supposed to be a histogram, but that's too expensive without server-side aggregation.
            init_duration_millis: Summary::new(Descriptor {
                name: "faas.init_duration",
                description: "Duration of Lambda execution environment initialization",
                unit: "ms",
            }),
            // This metric is not defined by OTEL
            restore_duration_millis: Summary::new(Descriptor {
                name: "faas.restore_duration",
                description: "Duration of Lambda execution environment restoration",
                unit: "ms",
            }),
            // https://github.com/open-telemetry/semantic-conventions/blob/6814d83dac7ebd6b29dada91c669a99275f674c8/docs/faas/faas-metrics.md?plain=1#L42
            // According to OTEL semantic convention this is supposed to be a histogram, but that's too expensive without server-side aggregation.
            invoke_duration_millis: Summary::new(Descriptor {
                name: "faas.invoke_duration",
                description: "Duration of Lambda function code execution",
                unit: "ms",
            }),
            // This metric is not defined by OTEL
            overhead_duration_millis: Summary::new(Descriptor {
                name: "faas.overhead_duration",
                description: "Duration of Lambda extension code execution after Lambda function code execution finished",
                unit: "ms",
            }),
            // This metric is not defined by OTEL
            result_size_bytes: Summary::new(Descriptor {
                name: "faas.result_size",
                description: "Size of result produced by Lambda Function",
                unit: "B",
            }),
            // This metric is already calculated as a maximum by AWS.
            // So it makes sense for us to further aggregate max_memory_used_mb from multiple invocations by taking a maximum (as opposed to for example averaging them).
            //
            // https://github.com/open-telemetry/semantic-conventions/blob/6814d83dac7ebd6b29dada91c669a99275f674c8/docs/faas/faas-metrics.md?plain=1#L266
            // But the unit is wrong
            max_memory_used_mb: MaxGauge::new(Descriptor {
                name: "faas.mem_usage",
                description: "Largest amount of memory used by the lambda function (and lambda extensions)",
                unit: "MB",
            }),
            // This metric won't change over time for given instance
            // This metric is not defined in OTEL semantic conventions, but it's useful as a reference for faas.mem_usage.
            // The OTEL semantic conventions recommend adding the mem limit as an attribute to the resource, but that's not so useful.
            // Using "limit" in the name as that's recommended by OTEL docs: https://opentelemetry.io/docs/reference/specification/metrics/semantic_conventions/#instrument-naming
            memory_size_mb: MaxGauge::new(Descriptor {
                name: "faas.mem_limit",
                description: "Size of memory configured for the lambda function",
                unit: "MB",
            }),
        }
    }
}

pub fn update_metrics_after_report<S: ProcessingState>(
    metrics_state: &mut PlatformMetricsState,
    state: &S,
) {
    // all metrics are updated after report in order to maintain consistent timestamps between the metrics
    let metrics = &mut metrics_state.metrics;
    let report_data = match state.get_report_data() {
        Some(report) => report,
        None => panic!(
            "update_metrics_after_report has been called without state.report_data being set. This is a bug."
        ),
    };
    let out_of_memory = is_out_of_memory(report_data);
    let report_metrics = &report_data.metrics;
    let runtime_metrics = state
        .get_runtime_done_data()
        .and_then(|x| x.metrics.as_ref());

    update_coldstarts_metric(metrics, report_metrics);
    update_executions_and_status_metrics(metrics, &report_data.status, out_of_memory);
    update_duration_metrics(metrics, runtime_metrics, report_metrics);
    update_result_size_metric(metrics, runtime_metrics);
    update_memory_metrics(metrics, report_metrics);
    update_metrics_timestamp(metrics_state, &report_data.time);
}

fn update_coldstarts_metric(metrics: &mut PlatformMetrics, report_metrics: &ReportMetrics) {
    if report_metrics.init_duration_ms.is_some() || report_metrics.restore_duration_ms.is_some() {
        metrics.coldstarts.record(1);
    }
}

fn update_executions_and_status_metrics(
    metrics: &mut PlatformMetrics,
    status: &Status,
    out_of_memory: bool,
) {
    metrics.executions.record(1);

    match status {
        Status::Success => (),
        Status::Error if out_of_memory => metrics.ooms.record(1),
        Status::Error | Status::Failure => metrics.errors.record(1),
        Status::Timeout => metrics.timeouts.record(1),
    }
}

fn update_duration_metrics(
    metrics: &mut PlatformMetrics,
    runtime_metrics: Option<&RuntimeDoneMetrics>,
    report_metrics: &ReportMetrics,
) {
    if let Some(d) = report_metrics.init_duration_ms {
        metrics.init_duration_millis.record(d);
    }

    if let Some(d) = report_metrics.restore_duration_ms {
        metrics.restore_duration_millis.record(d);
    }

    if let Some(runtime_duration_ms) = runtime_metrics.map(|x| x.duration_ms) {
        metrics.invoke_duration_millis.record(runtime_duration_ms);
    }

    metrics
        .overhead_duration_millis
        .record(report_metrics.duration_ms - runtime_metrics.map_or(0.0, |x| x.duration_ms));
}

fn update_result_size_metric(
    metrics: &mut PlatformMetrics,
    runtime_metrics: Option<&RuntimeDoneMetrics>,
) {
    if let Some(bytes) = runtime_metrics.and_then(|x| x.produced_bytes) {
        metrics.result_size_bytes.record(bytes as f64);
    }
}

fn update_memory_metrics(metrics: &mut PlatformMetrics, report_metrics: &ReportMetrics) {
    metrics
        .max_memory_used_mb
        .record(report_metrics.max_memory_used_mb as i64);

    metrics
        .memory_size_mb
        .record(report_metrics.memory_size_mb as i64); // this won't change during the lifetime of the extension, but we want to keep reporting it
}

fn update_metrics_timestamp(metrics_state: &mut PlatformMetricsState, timestamp: &OffsetDateTime) {
    metrics_state.latest_data_timestamp_nanos = timestamp.unix_timestamp_nanos() as u64;
}

pub fn make_metrics_report(state: &mut PlatformMetricsState) -> Vec<Metric> {
    let start_timestamp_nanos = state.start_timestamp_nanos;
    let timestamp_nanos = state.latest_data_timestamp_nanos;

    let metrics = &mut state.metrics;

    let counters: Vec<Metric> = vec![
        &mut metrics.coldstarts,
        &mut metrics.executions,
        &mut metrics.errors,
        &mut metrics.ooms,
        &mut metrics.timeouts,
    ]
    .into_iter()
    .filter_map(|i| i.report(start_timestamp_nanos, timestamp_nanos))
    .collect();

    let summaries: Vec<Metric> = vec![
        &mut metrics.init_duration_millis,
        &mut metrics.restore_duration_millis,
        &mut metrics.invoke_duration_millis,
        &mut metrics.overhead_duration_millis,
        &mut metrics.result_size_bytes,
    ]
    .into_iter()
    .filter_map(|i| i.report(start_timestamp_nanos, timestamp_nanos))
    .collect();

    let maxes: Vec<Metric> = vec![&mut metrics.max_memory_used_mb, &mut metrics.memory_size_mb]
        .into_iter()
        .filter_map(|i| i.report(start_timestamp_nanos, timestamp_nanos))
        .collect();

    let report: Vec<Metric> = counters.into_iter().chain(summaries).chain(maxes).collect();

    trace!("Made metrics report containing {} metrics", report.len());

    report
}
