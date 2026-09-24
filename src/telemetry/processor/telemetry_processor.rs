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

use super::epsagon_trace_processor::*;
use super::log_processor::*;
use super::platform_metrics;
use super::platform_metrics::PlatformMetricsState;
use super::span_processor;
use super::span_processor::*;
use crate::Error;
use crate::config::app_config::{LogMode, OtelMetricsMode, TelemetryProcessorConfig, TracingMode};
use crate::proto::opentelemetry::proto::metrics::v1::ResourceMetrics;
use crate::proto::opentelemetry::proto::trace::v1::status::StatusCode;
use crate::telemetry::telemetry_service::OutputBuffers;
use crate::telemetry::*;
use lambda_extension::LambdaTelemetry;
use lambda_extension::LambdaTelemetryRecord::*;
use otel_metrics::OtelMetricsAccumulator;
use std::mem::take;

#[rustfmt::skip]
#[allow(clippy::collapsible_if)]
pub fn process_telemetry(
    config: &TelemetryProcessorConfig,
    event: LambdaTelemetry,
    state: &mut InvocationProcessingState,
    metrics_state: &mut PlatformMetricsState,
    output_buffers: &mut OutputBuffers,
) -> Result<(), Error> {

    let time = event_timestamp(&event)?;

    match event.record {
        Function(log_text) => {
            if config.log_mode == LogMode::Structured {
                match detect_epsagon_trace(log_text) {
                    EpsagonTraceDetectionResult::EpsagonTraceJson(trace) =>
                        output_buffers.epsagon_traces_buffer.push(trace),
                    EpsagonTraceDetectionResult::RegularLog(log_text) => {
                        if state.delaying_logs {
                            // Emitting logs is delayed until after the invocation span is received from the instrumentation code. This way logs can be correlated with its span_id.
                            state.logs.push(RawFunctionLog { log_text, time })
                        } else {
                            output_buffers.logs_buffer.push(make_function_log(config, state, log_text, time)?)
                        }
                    }
                }
            }
        }
        Extension(_) => (),
        PlatformInitStart { initialization_type, phase, runtime_version, runtime_version_arn } => {
            state.init_span_id = Some(random_span_id()); // AWS has a span id for init but it doesn't share it with us
            state.init_start_data = Some(PlatformInitStartData { time, initialization_type, phase, runtime_version, runtime_version_arn })
        }
        PlatformInitRuntimeDone { initialization_type, phase, status, error_type, spans } => {
            state.init_runtime_done_data = Some(PlatformInitRuntimeDoneData { time, initialization_type, phase, status, error_type, spans })
        }
        PlatformInitReport { initialization_type, phase, metrics, spans } => {
            let init_report_data = PlatformInitReportData { time, initialization_type, phase, metrics, spans };

            if let PlatformMetricsState::V2(metrics_state) = metrics_state {
                platform_metrics::v2::update_metrics_with_init_report(metrics_state, &init_report_data);
            }

            state.init_report_data = Some(init_report_data)
        }
        PlatformStart { request_id, version, tracing } => {
            let platform_start_data = PlatformStartData { time, request_id, version, tracing };
            if config.log_mode == LogMode::Structured && config.platform_log_set.start && !state.delaying_logs {
                let log = make_platform_start_log(config, state, platform_start_data.clone())?;
                output_buffers.logs_buffer.push(log);
            }
            state.start_data = Some(platform_start_data)
        }
        PlatformRuntimeDone { request_id, status, error_type, metrics, spans, tracing } => {
            let runtime_done_data = PlatformRuntimeDoneData { time, request_id, status, error_type, metrics, spans, tracing };

            if config.log_mode == LogMode::Structured && state.delaying_logs {
                // The runtime is done so we should give up on waiting for the invocation span and just emit the logs
                // TODO we could process these logs earlier when early invocation span is available
                state.delaying_logs = false;
                process_delayed_logs(state, config, output_buffers)?;
            }

            let status = state.invocation_span()
                .and_then(|span| span.status.as_ref())
                .filter(|status| status.code == StatusCode::Error as i32);

            let handler_status = status.map(|_| "error".to_owned());
            let handler_status_description = status.map(|x| x.message.clone());

            if config.log_mode == LogMode::Structured && config.platform_log_set.runtime_done {
                let runtime_done_log = make_platform_runtime_done_log(config, state, &runtime_done_data, handler_status, handler_status_description)?;
                output_buffers.logs_buffer.push(runtime_done_log);
            }

            if let PlatformMetricsState::V2(metrics_state) = metrics_state {
                platform_metrics::v2::update_metrics_with_runtime_done(metrics_state, &runtime_done_data);
            }

            // state.runtime_done_data needs to be set before calling make_spans_after_invocation
            state.runtime_done_data = Some(runtime_done_data);

            if state.tracing_state.should_produce_spans {
                if config.tracing_mode == TracingMode::OtelInstrumentation {
                    let special_function_spans = emit_trigger_and_invocation_spans(state);
                    output_buffers.add_function_scope_spans(special_function_spans);
                    let mut new_spans = make_spans_after_invocation_in_otel_mode(state);
                    output_buffers.spans_buffer.append(&mut new_spans);
                } else if config.tracing_mode == TracingMode::TelemetryApi {
                    let mut new_spans = make_spans_after_invocation(state);
                    output_buffers.spans_buffer.append(&mut new_spans);
                }
            }
        }
        PlatformReport { request_id, status, error_type, metrics, spans, tracing, } => {
            let report_data = PlatformReportData { time, request_id, status, error_type, metrics, spans, tracing };

            if config.log_mode == LogMode::Structured {
                // This is needed to handle any logs that have been produced after runtimeDone event. This can happen if they are produced by some background task that continues running after handler returns.
                for log in take(&mut state.logs) {
                    output_buffers.logs_buffer.push(make_function_log(config, state, log.log_text, log.time)?);
                }

                if config.platform_log_set.report {
                    let log = make_platform_report_log(config, state, &report_data)?;
                    output_buffers.logs_buffer.push(log);
                }
            }

            if let PlatformMetricsState::V2(metrics_state) = metrics_state {
                platform_metrics::v2::update_metrics_with_report(metrics_state, &report_data);
            }

            state.report_data = Some(report_data);

            if state.tracing_state.should_produce_spans {
                if config.tracing_mode == TracingMode::TelemetryApi {
                    let mut new_spans = make_spans_after_report(state);
                    output_buffers.spans_buffer.append(&mut new_spans);
                }
            }

            if let PlatformMetricsState::V1(metrics_state) = metrics_state {
                platform_metrics::v1::update_metrics_after_report(metrics_state, state)
            }
        }
        PlatformExtension { name: _, state: _, events: _ } => (),
        PlatformTelemetrySubscription { name: _, state: _, types: _ } => (),
        PlatformLogsDropped { reason: _, dropped_records: _, dropped_bytes: _ } => (),
    }
    Ok(())
}

// TODO Could the degraded mode be "less-degraded"?
#[rustfmt::skip]
#[allow(clippy::collapsible_match)]
pub fn process_telemetry_in_degraded_mode(
    config: &TelemetryProcessorConfig,
    event: LambdaTelemetry,
    state: &mut DegradedProcessingState,
    metrics_state: &mut PlatformMetricsState,
    output_buffers: &mut OutputBuffers,
) -> Result<(), Error> {
    let time = event_timestamp(&event)?;

    match event.record {
        Function(log_text) => {
            if config.log_mode == LogMode::Structured {
                match detect_epsagon_trace(log_text) {
                    EpsagonTraceDetectionResult::EpsagonTraceJson(trace) => 
                        output_buffers.epsagon_traces_buffer.push(trace),
                    EpsagonTraceDetectionResult::RegularLog(log_text) => 
                        output_buffers.logs_buffer.push(make_function_log(config, state, log_text, time)?),
                }
            }
        }
        PlatformInitRuntimeDone { initialization_type, phase, status, error_type, spans } => {
            state.init_runtime_done_data = Some(PlatformInitRuntimeDoneData { time, initialization_type, phase, status, error_type, spans })
        }
        PlatformInitReport { initialization_type, phase, metrics, spans } => {
            state.init_report_data = Some(PlatformInitReportData { time, initialization_type, phase, metrics, spans });
            if config.tracing_mode != TracingMode::Disabled {
                let mut new_spans = make_degraded_spans_after_init(state);
                output_buffers.spans_buffer.append(&mut new_spans);
            }
        }
        PlatformStart { request_id, version, tracing } => {
            let platform_start_data = PlatformStartData { time, request_id, version, tracing };
            if config.log_mode == LogMode::Structured && config.platform_log_set.start {
                let log = make_platform_start_log(config, state, platform_start_data)?;
                output_buffers.logs_buffer.push(log);
            }
        }
        PlatformRuntimeDone { request_id, status, error_type, metrics, spans, tracing } => {
            let runtime_done_data = PlatformRuntimeDoneData { time, request_id, status, error_type, metrics, spans, tracing };
            if config.log_mode == LogMode::Structured && config.platform_log_set.runtime_done {
                let log = make_platform_runtime_done_log(config, state, &runtime_done_data, None, None)?;
                output_buffers.logs_buffer.push(log);
            }
        }
        PlatformReport { request_id, status, error_type, metrics, spans, tracing, } => {
            let report_data = PlatformReportData { time, request_id, status, error_type, metrics, spans, tracing };
            if config.log_mode == LogMode::Structured && config.platform_log_set.report {
                let log = make_platform_report_log(config, state, &report_data)?;
                output_buffers.logs_buffer.push(log);
            }

            if let PlatformMetricsState::V2(metrics_state) = metrics_state {
                platform_metrics::v2::update_metrics_with_report(metrics_state, &report_data);
            }

            state.report_data = Some(report_data);

            if let PlatformMetricsState::V1(metrics_state) = metrics_state {
                platform_metrics::v1::update_metrics_after_report(metrics_state, state)
            }
        }
        _ => ()
    }
    Ok(())
}

pub fn process_function_metrics(
    function_metrics: Vec<ResourceMetrics>,
    config: &TelemetryProcessorConfig,
    metrics_accumulator: &mut OtelMetricsAccumulator,
    output_buffers: &mut OutputBuffers,
) {
    match config.otel_metrics_mode {
        OtelMetricsMode::Disabled => (),
        OtelMetricsMode::Direct => output_buffers
            .function_metrics_buffer
            .extend(function_metrics),
        OtelMetricsMode::Processed => function_metrics
            .into_iter()
            .flat_map(|rs| rs.scope_metrics.into_iter())
            .for_each(|sm| metrics_accumulator.accumulate(sm.metrics)),
    }
}

pub fn process_function_spans(
    state: &mut InvocationProcessingState,
    function_spans: Vec<ResourceSpans>,
    config: &TelemetryProcessorConfig,
    output_buffers: &mut OutputBuffers,
) -> Result<(), Error> {
    let function_spans =
        span_processor::process_function_spans(&mut state.tracing_state, function_spans, config);

    if state.tracing_state.should_produce_spans
        && config.tracing_mode == TracingMode::OtelInstrumentation
    {
        output_buffers.add_function_resource_spans(function_spans);
    }

    if config.log_mode == LogMode::Structured
        && state.delaying_logs
        && state.tracing_state.cx_early_invocation_span.is_some()
    {
        // We have the invocation span id / trace id, so we can start processing any buffered logs and future logs
        // If we never get here (non-CX otel instrumentation, or function crash/timeout), then we will emit logs on runtime done
        state.delaying_logs = false;
        process_delayed_logs(state, config, output_buffers)?;
    }
    Ok(())
}

fn process_delayed_logs(
    state: &mut InvocationProcessingState,
    config: &TelemetryProcessorConfig,
    output_buffers: &mut OutputBuffers,
) -> Result<(), Error> {
    if let Some(platform_start_data) = state.start_data.as_ref()
        && config.platform_log_set.start
    {
        let log = make_platform_start_log(config, state, platform_start_data.clone())?;
        output_buffers.logs_buffer.push(log);
    }

    for log in take(&mut state.logs) {
        output_buffers
            .logs_buffer
            .push(make_function_log(config, state, log.log_text, log.time)?);
    }

    Ok(())
}
