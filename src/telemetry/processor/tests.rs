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

use super::PlatformMetricsState;
use super::telemetry_processor::process_telemetry;
use crate::config::app_config::{
    AttributeConfig, AttributeConfigs, AttributeExclusionMode, BuiltInAttributeSet,
    LogMetadataConfig, LogMode, LogsMetadataMode, OtelMetricsMode, PlatformEventLogSet,
    PlatformLogsConfig, PlatformMetricsMode, TelemetryProcessorConfig, TraceSamplingMode,
    TracingMode,
};
use crate::proto::opentelemetry::proto::common::v1::AnyValue;
use crate::proto::opentelemetry::proto::common::v1::any_value::Value;
use crate::proto::opentelemetry::proto::logs::v1::LogRecord;
use crate::proto::opentelemetry::proto::metrics::v1::{metric, number_data_point};
use crate::proto::opentelemetry::proto::resource::v1::Resource;
use crate::proto::opentelemetry::proto::trace::v1::span::SpanKind;
use crate::proto::opentelemetry::proto::trace::v1::status::StatusCode;
use crate::proto::opentelemetry::proto::trace::v1::{self as otel_trace, Status};
use crate::proto::opentelemetry::proto::trace::v1::{ResourceSpans, ScopeSpans, Span};
use crate::telemetry::function_context::FunctionContext;
use crate::telemetry::lambda_function_arn::LambdaFunctionArn;
use crate::telemetry::processor::telemetry_processor::process_function_spans;
use crate::telemetry::telemetry_service::OutputBuffers;
use crate::telemetry::{
    InvocationContext, InvocationProcessingState, find_attribute, random_span_id, random_trace_id,
    string_attribute,
};
use core::panic;
use lambda_extension::{LambdaTelemetry, LambdaTelemetryRecord, ReportMetrics};
use std::collections::HashMap;
use std::iter::repeat_with;
use std::mem::take;
use std::sync::Arc;
use time::OffsetDateTime;

fn test_invocation_processing_state(tracing_mode: TracingMode) -> InvocationProcessingState {
    InvocationProcessingState::new(
        Arc::new(test_function_context()),
        test_invocation_context(),
        TraceSamplingMode::All,
        tracing_mode,
    )
}

fn test_function_context() -> FunctionContext {
    let arn = test_arn();
    FunctionContext {
        arn: arn.clone().without_version(),
        version_arn: arn.clone(),
        application_name: arn.account_id.clone(),
        subsystem_name: arn.function_name.clone(),
        service_name: arn.function_name.clone(),
        lambda_function_version: arn.version.unwrap(),
        lambda_instance_coralogix_id: "00000000".to_owned(),
        tags: None,
    }
}

fn test_invocation_context() -> InvocationContext {
    InvocationContext {
        invoked_arn: test_arn(),
        request_id: "0000".to_owned(),
        trace_id: test_trace_id(),
        invocation_span_id: repeat_with(|| 0).take(8).collect(),
        sampled: true,
    }
}

fn test_trace_id() -> Vec<u8> {
    repeat_with(|| 0).take(16).collect()
}

fn test_arn() -> LambdaFunctionArn {
    "arn:aws:lambda:eu-west-1:200000000000:function:lambda-telemetry-test:$LATEST"
        .parse::<LambdaFunctionArn>()
        .unwrap()
}

fn platform_start_event() -> LambdaTelemetry {
    LambdaTelemetry {
        time: chrono::offset::Utc::now(),
        record: LambdaTelemetryRecord::PlatformStart {
            request_id: "0".to_owned(),
            version: None,
            tracing: None,
        },
    }
}

fn platform_runtime_done_event() -> LambdaTelemetry {
    LambdaTelemetry {
        time: chrono::offset::Utc::now(),
        record: LambdaTelemetryRecord::PlatformRuntimeDone {
            request_id: "0".to_owned(),
            status: lambda_extension::Status::Success,
            error_type: None,
            metrics: None,
            spans: Vec::new(),
            tracing: None,
        },
    }
}

fn platform_report_event() -> LambdaTelemetry {
    LambdaTelemetry {
        time: chrono::offset::Utc::now(),
        record: LambdaTelemetryRecord::PlatformReport {
            request_id: "0".to_owned(),
            status: lambda_extension::Status::Success,
            error_type: None,
            metrics: ReportMetrics {
                duration_ms: 10.0,
                billed_duration_ms: 10,
                memory_size_mb: 128,
                max_memory_used_mb: 100,
                init_duration_ms: None,
                restore_duration_ms: None,
            },
            spans: Vec::new(),
            tracing: None,
        },
    }
}

fn function_log_event() -> LambdaTelemetry {
    LambdaTelemetry {
        time: chrono::offset::Utc::now(),
        record: LambdaTelemetryRecord::Function("test function log".to_owned()),
    }
}

fn get_string_value<'a>(parent: &'a AnyValue, key: &str) -> Option<&'a str> {
    as_string(get_value(parent, key)?)
}

fn get_bool_value(parent: &AnyValue, key: &str) -> Option<bool> {
    as_bool(get_value(parent, key)?)
}

fn get_value<'a>(parent: &'a AnyValue, key: &str) -> Option<&'a AnyValue> {
    match parent.value.as_ref()? {
        Value::KvlistValue(kv_list) => kv_list.values.iter().find(|x| x.key == key)?.value.as_ref(),
        _ => None,
    }
}

fn as_string(value: &AnyValue) -> Option<&str> {
    match value.value.as_ref()? {
        Value::StringValue(s) => Some(s.as_str()),
        _ => None,
    }
}

fn as_bool(value: &AnyValue) -> Option<bool> {
    match value.value.as_ref()? {
        Value::BoolValue(b) => Some(*b),
        _ => None,
    }
}

fn now_nanos() -> u64 {
    OffsetDateTime::now_utc().unix_timestamp_nanos() as u64
}

fn default_config() -> TelemetryProcessorConfig {
    TelemetryProcessorConfig {
        logs_metadata_mode: LogsMetadataMode::Enabled(LogMetadataConfig {
            include_trace_ref: true,
            include_execution: true,
            include_invocation_id: false,
        }),
        log_mode: LogMode::Structured,
        platform_log_set: PlatformEventLogSet::all(),
        platform_logs: PlatformLogsConfig {
            include_request_id: true,
            hide_default_values: false,
        },
        platform_metrics_mode: PlatformMetricsMode::PlatformReport,
        otel_metrics_mode: OtelMetricsMode::Direct,
        tracing_mode: TracingMode::TelemetryApi,
        trace_sampling_mode: TraceSamplingMode::All,
        message_size_limit: 30_000,
        excluded_span_attributes: AttributeExclusionMode::Disabled,
        resource_attributes: AttributeConfigs {
            logs: AttributeConfig {
                built_in: BuiltInAttributeSet::all(),
                extra: HashMap::new(),
            },
            traces: AttributeConfig {
                built_in: BuiltInAttributeSet::all(),
                extra: HashMap::new(),
            },
            metrics: AttributeConfig {
                built_in: BuiltInAttributeSet::all(),
                extra: HashMap::new(),
            },
        },
    }
}

fn assert_attribute(span: &Span, key: &str, expected_value: &str) {
    if let Some(actual_value) = find_attribute(&span.attributes, key) {
        match actual_value {
            Value::StringValue(actual_value) => {
                if actual_value.as_str() != expected_value {
                    panic!(
                        "Attribute '{}' of span {}/{} is expected to be '{}' but was '{}'",
                        key,
                        hex::encode(&span.span_id),
                        span.name,
                        expected_value,
                        actual_value
                    )
                }
            }
            other => panic!(
                "Attribute '{}' of span {}/{} is expected to be a string but was '{:?}'",
                key,
                hex::encode(&span.span_id),
                span.name,
                other
            ),
        }
    } else {
        panic!(
            "Span {}/{} didn't contain attribute '{}'",
            hex::encode(&span.span_id),
            span.name,
            key
        );
    }
}

// Merges scope attributes into span attributes. It doesn't matter for Coralogix, so the tests also don't care.
fn flatten_scopes(ss: Vec<ScopeSpans>) -> Vec<Span> {
    ss.into_iter()
        .flat_map(|ss| {
            ss.spans.into_iter().map(move |mut s| {
                s.attributes
                    .extend(ss.scope.iter().flat_map(|scope| scope.attributes.clone()));
                s
            })
        })
        .collect()
}

fn assert_span<'a>(spans: &'a [Span], span_id: Vec<u8>, span_name: &str) -> &'a Span {
    match spans.iter().find(|s| s.span_id == span_id) {
        Some(span) => {
            if span.name == span_name {
                span
            } else {
                panic!(
                    "Span {} is expected to have name '{}', but it's name is '{}'",
                    hex::encode(&span_id),
                    span_name,
                    span.name
                );
            }
        }
        None => panic!("Span {}/{} is missing", hex::encode(&span_id), span_name),
    }
}

fn assert_function_log(log: &LogRecord) {
    let body = log.body.as_ref().unwrap();
    let message = get_string_value(body, "message").unwrap();
    assert_eq!(message, "test function log")
}

fn assert_event_log<'a>(log: &'a LogRecord, expected_event_type: &str) -> &'a AnyValue {
    let body = log.body.as_ref().unwrap();
    let actual_event_type = get_string_value(body, "platform_event_type").unwrap();
    assert_eq!(actual_event_type, expected_event_type);
    get_value(body, "event").unwrap()
}

// Wraps all the context / state /configuration data need to run telemetry processor.
// This is here to reduce boilerplate code in the tests.
struct TelemetryProcessorSetup {
    pub config: TelemetryProcessorConfig,
    pub state: InvocationProcessingState,
    pub output: OutputBuffers,
    pub metrics_state: PlatformMetricsState,
}

impl TelemetryProcessorSetup {
    fn new(config: TelemetryProcessorConfig) -> TelemetryProcessorSetup {
        let tracing_mode = config.tracing_mode;
        let platform_metrics_mode = config.platform_metrics_mode;
        TelemetryProcessorSetup {
            config,
            state: test_invocation_processing_state(tracing_mode),
            output: OutputBuffers::default(),
            metrics_state: PlatformMetricsState::for_mode(platform_metrics_mode),
        }
    }

    fn process_telemetry(&mut self, event: LambdaTelemetry) {
        process_telemetry(
            &self.config,
            event,
            &mut self.state,
            &mut self.metrics_state,
            &mut self.output,
        )
        .unwrap()
    }

    fn process_function_spans(&mut self, function_spans: Vec<ResourceSpans>) {
        process_function_spans(
            &mut self.state,
            function_spans,
            &self.config,
            &mut self.output,
        )
        .unwrap()
    }
}

#[test]
fn context_and_invocation_spans_are_emitted_in_telemetry_api_tracing_mode_and_logs_correlate_with_invocation_span()
 {
    let mut processor = TelemetryProcessorSetup::new(default_config());

    processor.process_telemetry(platform_start_event());
    processor.process_telemetry(function_log_event());

    let resource_spans = vec![ResourceSpans {
        resource: Some(Resource {
            attributes: vec![],
            dropped_attributes_count: 0,
        }),
        scope_spans: vec![ScopeSpans {
            spans: vec![Span {
                trace_id: test_trace_id(),
                span_id: vec![0, 0, 0, 0, 0, 0, 0, 1],
                parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 0],
                name: "trigger".to_owned(),
                ..Span::default()
            }],
            ..ScopeSpans::default()
        }],
        ..ResourceSpans::default()
    }];
    processor.process_function_spans(resource_spans);

    processor.process_telemetry(platform_runtime_done_event());

    let function_spans = processor.output.take_function_spans();
    assert_eq!(function_spans.len(), 0);

    let spans = take(&mut processor.output.spans_buffer);
    assert_eq!(spans.len(), 1);
    let invocation_span = &spans[0];
    assert_eq!(
        invocation_span.name,
        format!("{} invocation", test_arn().function_name)
    );

    // Logs correlate with the invocation span
    let logs = take(&mut processor.output.logs_buffer);
    assert_eq!(logs.len(), 3);
    assert_event_log(&logs[0], "start");
    assert_function_log(&logs[1]);
    assert_event_log(&logs[2], "runtime_done");
    for log in logs {
        assert_eq!(log.span_id, invocation_span.span_id);
        assert_eq!(log.trace_id, invocation_span.trace_id);
    }

    // This happens earliest during next invocation
    processor.process_telemetry(platform_report_event());

    let spans = take(&mut processor.output.spans_buffer);
    assert_eq!(spans.len(), 1);
    assert_eq!(
        spans[0].name,
        format!("{} context", test_arn().function_name)
    );
}

#[test]
fn otel_instrumentation_spans_processing() {
    let mut processor = TelemetryProcessorSetup::new(TelemetryProcessorConfig {
        tracing_mode: TracingMode::OtelInstrumentation,
        ..default_config()
    });

    processor.process_telemetry(platform_start_event());
    processor.process_telemetry(function_log_event());

    let resource_spans = vec![ResourceSpans {
        resource: Some(Resource {
            attributes: vec![string_attribute("telemetry.sdk.language", "nodejs")],
            dropped_attributes_count: 0,
        }),
        scope_spans: vec![ScopeSpans {
            spans: vec![
                Span {
                    // trigger span shouldn't be misrecognized as invocation span even if it's a Server span
                    trace_id: test_trace_id(),
                    span_id: vec![0, 0, 0, 0, 0, 0, 0, 1],
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 0],
                    name: "trigger".to_owned(),
                    kind: (SpanKind::Server as i32),
                    attributes: vec![string_attribute("faas.trigger", "aaa")],
                    ..Span::default()
                },
                Span {
                    // invocation span is recognized and logs correlate with it
                    trace_id: test_trace_id(),
                    span_id: vec![0, 0, 0, 0, 0, 0, 0, 2],
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 1],
                    name: "invocation".to_owned(),
                    kind: (SpanKind::Server as i32), // it has to be a server span
                    ..Span::default()
                },
                Span {
                    // all other spans are just forwarded
                    trace_id: test_trace_id(),
                    span_id: vec![0, 0, 0, 0, 0, 0, 0, 3],
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 2],
                    name: "internal".to_owned(),
                    kind: (SpanKind::Internal as i32),
                    ..Span::default()
                },
            ],
            ..ScopeSpans::default()
        }],
        ..ResourceSpans::default()
    }];
    processor.process_function_spans(resource_spans);

    processor.process_telemetry(platform_runtime_done_event());

    let spans = flatten_scopes(processor.output.take_function_spans());
    assert_eq!(spans.len(), 3);

    assert_span(&spans, vec![0, 0, 0, 0, 0, 0, 0, 1], "trigger");
    let invocation_span = assert_span(&spans, vec![0, 0, 0, 0, 0, 0, 0, 2], "invocation");
    assert_span(&spans, vec![0, 0, 0, 0, 0, 0, 0, 3], "internal");

    // invocation span has `telemetry.sdk.language` attribute (originating from resource attributes)
    assert_attribute(invocation_span, "telemetry.sdk.language", "nodejs");

    // Logs correlate with the function invocation span
    let logs = take(&mut processor.output.logs_buffer);
    assert_eq!(logs.len(), 3);
    assert_event_log(&logs[0], "start");
    assert_function_log(&logs[1]);
    assert_event_log(&logs[2], "runtime_done");
    for log in logs {
        assert_eq!(log.span_id, invocation_span.span_id);
        assert_eq!(log.trace_id, invocation_span.trace_id);
    }

    // This happens earliest during next invocation
    processor.process_telemetry(platform_report_event());

    // No platform spans are reported
    assert_eq!(processor.output.spans_buffer.len(), 0);
}

#[test]
fn cx_otel_instrumentation_spans_processing() {
    let mut processor = TelemetryProcessorSetup::new(TelemetryProcessorConfig {
        tracing_mode: TracingMode::OtelInstrumentation,
        ..default_config()
    });

    processor.process_telemetry(platform_start_event());

    // Typically we don't expect logs before the early spans, but let's test it anyway
    processor.process_telemetry(function_log_event());

    // Spans sent before function handler starts execution
    let resource_spans = vec![ResourceSpans {
        resource: Some(Resource {
            attributes: vec![string_attribute("telemetry.sdk.language", "nodejs")],
            dropped_attributes_count: 0,
        }),
        scope_spans: vec![ScopeSpans {
            spans: vec![
                Span {
                    // early trigger span
                    trace_id: random_trace_id(), // this trace id will be ignored because `cx.internal.trace.id` is specified
                    span_id: random_span_id(), // this span id will be ignored because `cx.internal.span.id` is specified
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 0],
                    name: "trigger".to_owned(),
                    kind: (SpanKind::Server as i32),
                    start_time_unix_nano: now_nanos(),
                    end_time_unix_nano: now_nanos(),
                    attributes: vec![
                        string_attribute("cx.internal.span.state", "early"),
                        string_attribute("cx.internal.span.role", "trigger"),
                        string_attribute(
                            "cx.internal.trace.id",
                            "00000000000000000000000000000000",
                        ),
                        string_attribute("cx.internal.span.id", "0000000000000001"),
                        string_attribute("faas.trigger", "aaa"),
                    ],
                    ..Span::default()
                },
                Span {
                    // early invocation span
                    trace_id: random_trace_id(), // this trace id will be ignored because `cx.internal.trace.id` is specified
                    span_id: random_span_id(), // this span id will be ignored because `cx.internal.span.id` is specified
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 1],
                    name: "invocation".to_owned(),
                    kind: (SpanKind::Server as i32), // it has to be a server span
                    attributes: vec![
                        string_attribute("cx.internal.span.state", "early"),
                        string_attribute("cx.internal.span.role", "invocation"),
                        string_attribute(
                            "cx.internal.trace.id",
                            "00000000000000000000000000000000",
                        ),
                        string_attribute("cx.internal.span.id", "0000000000000002"),
                    ],
                    ..Span::default()
                },
            ],
            ..ScopeSpans::default()
        }],
        ..ResourceSpans::default()
    }];
    processor.process_function_spans(resource_spans);

    // A log delivered during the handler execution
    processor.process_telemetry(function_log_event());

    // Once the early invocation span is processed, logs are processed.
    let logs = take(&mut processor.output.logs_buffer);
    assert_eq!(logs.len(), 3);
    assert_event_log(&logs[0], "start");
    assert_function_log(&logs[1]);
    assert_function_log(&logs[2]);
    for log in logs.iter() {
        assert_eq!(log.span_id, vec![0, 0, 0, 0, 0, 0, 0, 2]);
        assert_eq!(log.trace_id, test_trace_id());
    }

    // Spans delivered after the handler completes
    let resource_spans = vec![ResourceSpans {
        resource: Some(Resource {
            attributes: vec![string_attribute("telemetry.sdk.language", "nodejs")],
            dropped_attributes_count: 0,
        }),
        scope_spans: vec![ScopeSpans {
            spans: vec![
                Span {
                    // trigger span
                    trace_id: test_trace_id(),
                    span_id: vec![0, 0, 0, 0, 0, 0, 0, 1],
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 0],
                    name: "trigger".to_owned(),
                    kind: (SpanKind::Server as i32),
                    attributes: vec![
                        string_attribute("cx.internal.span.role", "trigger"),
                        string_attribute("faas.trigger", "aaa"),
                    ],
                    ..Span::default()
                },
                Span {
                    // invocation span
                    trace_id: test_trace_id(),
                    span_id: vec![0, 0, 0, 0, 0, 0, 0, 2],
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 1],
                    name: "invocation".to_owned(),
                    kind: (SpanKind::Server as i32), // it has to be a server span
                    attributes: vec![string_attribute("cx.internal.span.role", "invocation")],
                    ..Span::default()
                },
                Span {
                    // all other spans are just forwarded
                    trace_id: test_trace_id(),
                    span_id: vec![0, 0, 0, 0, 0, 0, 0, 3],
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 2],
                    name: "internal".to_owned(),
                    kind: (SpanKind::Internal as i32),
                    ..Span::default()
                },
            ],
            ..ScopeSpans::default()
        }],
        ..ResourceSpans::default()
    }];
    processor.process_function_spans(resource_spans);

    // A log produced by the handler, but delivered later because of the telemetry api buffering
    processor.process_telemetry(function_log_event());

    processor.process_telemetry(platform_runtime_done_event());

    let spans = flatten_scopes(processor.output.take_function_spans());
    assert_eq!(spans.len(), 3); // the early spans are ignored because their proper versions have been received

    assert_span(&spans, vec![0, 0, 0, 0, 0, 0, 0, 1], "trigger");
    let invocation_span = assert_span(&spans, vec![0, 0, 0, 0, 0, 0, 0, 2], "invocation");
    assert_span(&spans, vec![0, 0, 0, 0, 0, 0, 0, 3], "internal");

    // invocation span has `telemetry.sdk.language` attribute (originating from resource attributes)
    assert_attribute(invocation_span, "telemetry.sdk.language", "nodejs");

    // The remaining logs are processed too
    // Logs correlate with the function invocation span
    let logs = take(&mut processor.output.logs_buffer);
    assert_eq!(logs.len(), 2);
    assert_function_log(&logs[0]);
    assert_event_log(&logs[1], "runtime_done");
    for log in logs.iter() {
        assert_eq!(log.span_id, invocation_span.span_id);
        assert_eq!(log.trace_id, test_trace_id());
    }

    // This happens earliest during next invocation
    processor.process_telemetry(platform_report_event());

    // No platform spans are reported
    assert_eq!(processor.output.spans_buffer.len(), 0);
}

#[test]
fn cx_otel_instrumentation_spans_processing_in_case_of_a_timeout() {
    let mut processor = TelemetryProcessorSetup::new(TelemetryProcessorConfig {
        tracing_mode: TracingMode::OtelInstrumentation,
        ..default_config()
    });

    processor.process_telemetry(platform_start_event());
    processor.process_telemetry(function_log_event());

    let resource_spans = vec![ResourceSpans {
        resource: Some(Resource {
            attributes: vec![string_attribute("telemetry.sdk.language", "nodejs")],
            dropped_attributes_count: 0,
        }),
        scope_spans: vec![ScopeSpans {
            spans: vec![
                Span {
                    // early trigger span
                    trace_id: random_trace_id(), // this trace id will be ignored because `cx.internal.trace.id` is specified
                    span_id: random_span_id(), // this span id will be ignored because `cx.internal.span.id` is specified
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 0],
                    name: "trigger".to_owned(),
                    kind: (SpanKind::Server as i32),
                    attributes: vec![
                        string_attribute("cx.internal.span.state", "early"),
                        string_attribute("cx.internal.span.role", "trigger"),
                        string_attribute(
                            "cx.internal.trace.id",
                            "00000000000000000000000000000000",
                        ),
                        string_attribute("cx.internal.span.id", "0000000000000001"),
                        string_attribute("faas.trigger", "aaa"),
                    ],
                    ..Span::default()
                },
                Span {
                    // early invocation span
                    trace_id: random_trace_id(), // this trace id will be ignored because `cx.internal.trace.id` is specified
                    span_id: random_span_id(), // this span id will be ignored because `cx.internal.span.id` is specified
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 1],
                    name: "invocation".to_owned(),
                    kind: (SpanKind::Server as i32), // it has to be a server span
                    attributes: vec![
                        string_attribute("cx.internal.span.state", "early"),
                        string_attribute("cx.internal.span.role", "invocation"),
                        string_attribute(
                            "cx.internal.trace.id",
                            "00000000000000000000000000000000",
                        ),
                        string_attribute("cx.internal.span.id", "0000000000000002"),
                    ],
                    ..Span::default()
                },
                Span {
                    // all other spans are just forwarded
                    trace_id: test_trace_id(),
                    span_id: vec![0, 0, 0, 0, 0, 0, 0, 3],
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 2],
                    name: "internal".to_owned(),
                    kind: (SpanKind::Internal as i32),
                    ..Span::default()
                },
            ],
            ..ScopeSpans::default()
        }],
        ..ResourceSpans::default()
    }];
    processor.process_function_spans(resource_spans);

    processor.process_telemetry(LambdaTelemetry {
        time: chrono::offset::Utc::now(),
        record: LambdaTelemetryRecord::PlatformRuntimeDone {
            request_id: "0".to_owned(),
            status: lambda_extension::Status::Timeout,
            error_type: None, // timeout comes with no error_type
            metrics: None,
            spans: Vec::new(),
            tracing: None,
        },
    });

    let spans = flatten_scopes(processor.output.take_function_spans());
    assert_eq!(spans.len(), 3); // invocation and trigger spans haven't been received so the early spans are used

    assert_span(&spans, vec![0, 0, 0, 0, 0, 0, 0, 1], "trigger");
    let invocation_span = assert_span(&spans, vec![0, 0, 0, 0, 0, 0, 0, 2], "invocation");
    assert_span(&spans, vec![0, 0, 0, 0, 0, 0, 0, 3], "internal");

    assert_eq!(
        invocation_span.status,
        Some(Status {
            message: "timeout".to_owned(),
            code: StatusCode::Error as i32
        })
    );

    // invocation span has `telemetry.sdk.language` attribute (originating from resource attributes)
    assert_attribute(invocation_span, "telemetry.sdk.language", "nodejs");

    // Logs correlate with the function invocation span
    let logs = take(&mut processor.output.logs_buffer);
    assert_eq!(logs.len(), 3);
    assert_event_log(&logs[0], "start");
    assert_function_log(&logs[1]);
    assert_event_log(&logs[2], "runtime_done");
    for log in logs {
        assert_eq!(log.span_id, invocation_span.span_id);
        assert_eq!(log.trace_id, invocation_span.trace_id);
    }

    // This happens earliest during next invocation
    processor.process_telemetry(platform_report_event());

    // No platform spans are reported
    assert_eq!(processor.output.spans_buffer.len(), 0);
}

#[test]
fn handler_status_indicates_an_error_when_the_main_function_span_indicates_an_error() {
    let mut processor = TelemetryProcessorSetup::new(TelemetryProcessorConfig {
        tracing_mode: TracingMode::OtelInstrumentation,
        ..default_config()
    });

    processor.process_telemetry(platform_start_event());

    let resource_spans = vec![ResourceSpans {
        scope_spans: vec![ScopeSpans {
            spans: vec![Span {
                trace_id: test_trace_id(),
                span_id: vec![0, 0, 0, 0, 0, 0, 0, 1],
                parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 0],
                name: "function".to_owned(),
                kind: (SpanKind::Server as i32),
                start_time_unix_nano: now_nanos(),
                end_time_unix_nano: now_nanos(),
                status: Some(otel_trace::Status {
                    message: "error message".to_owned(),
                    code: otel_trace::status::StatusCode::Error as i32,
                }),
                ..Span::default()
            }],
            ..ScopeSpans::default()
        }],
        ..ResourceSpans::default()
    }];
    processor.process_function_spans(resource_spans);

    processor.process_telemetry(function_log_event());
    processor.process_telemetry(platform_runtime_done_event());

    let spans = flatten_scopes(processor.output.take_function_spans());
    assert_eq!(spans.len(), 1);

    let logs = take(&mut processor.output.logs_buffer);
    assert_eq!(logs.len(), 3);
    assert_event_log(&logs[0], "start");
    assert_function_log(&logs[1]);
    let event = assert_event_log(&logs[2], "runtime_done");
    // handler status indicates an error
    let handler_status = get_string_value(event, "handler_status").unwrap();
    let handler_status_description = get_string_value(event, "handler_status_description").unwrap();
    assert_eq!(handler_status, "error");
    assert_eq!(handler_status_description, "error message");

    // Logs correlate with the function invocation span
    let function_invocation_span_id = vec![0, 0, 0, 0, 0, 0, 0, 1];
    for log in logs {
        assert_eq!(log.span_id, function_invocation_span_id);
        assert_eq!(log.trace_id, test_trace_id());
    }
}

#[test]
fn out_of_memory_is_set_when_runtime_fails_and_memory_usage_is_at_maximum() {
    let mut processor = TelemetryProcessorSetup::new(default_config());

    processor.process_telemetry(platform_start_event());

    processor.process_telemetry(LambdaTelemetry {
        time: chrono::offset::Utc::now(),
        record: LambdaTelemetryRecord::PlatformRuntimeDone {
            request_id: "0".to_owned(),
            status: lambda_extension::Status::Error,
            error_type: None, // timeout comes with no error_type
            metrics: None,
            spans: Vec::new(),
            tracing: None,
        },
    });

    processor.process_telemetry(LambdaTelemetry {
        time: chrono::offset::Utc::now(),
        record: LambdaTelemetryRecord::PlatformReport {
            request_id: "0".to_owned(),
            status: lambda_extension::Status::Error,
            error_type: None, // timeout comes with no error_type
            metrics: ReportMetrics {
                duration_ms: 1.0,
                billed_duration_ms: 1,
                memory_size_mb: 256,
                max_memory_used_mb: 256,
                init_duration_ms: None,
                restore_duration_ms: None,
            },
            spans: Vec::new(),
            tracing: None,
        },
    });

    // Report log has out_of_memory: true
    assert_eq!(processor.output.logs_buffer.len(), 3);
    let event = assert_event_log(&processor.output.logs_buffer[2], "report");
    let out_of_memory = get_bool_value(event, "out_of_memory").unwrap();
    assert!(out_of_memory);

    // OOM metric is reported
    let PlatformMetricsState::V1(metrics_state) = &mut processor.metrics_state else {
        panic!("Expected PlatformMetricsState::V1")
    };
    let oom_metric = metrics_state.metrics.ooms.report(0, 0);
    let Some(metric) = oom_metric else {
        panic!("Expected OOM metric to be reported")
    };
    let Some(metric::Data::Sum(data)) = metric.data else {
        panic!("Expected OOM metric to have data")
    };
    assert_eq!(data.data_points.len(), 1);
    assert_eq!(
        data.data_points[0].value,
        Some(number_data_point::Value::AsInt(1))
    )
}

#[test]
fn epsagon_traces_are_recognised() {
    let mut processor = TelemetryProcessorSetup::new(default_config());

    processor.process_telemetry(platform_start_event());

    processor.process_telemetry(
        LambdaTelemetry {
            time: chrono::offset::Utc::now(),
            record: LambdaTelemetryRecord::Function("EPSAGON_TRACE: eyJhcHBfbmFtZSI6ImVwc2Fnb24tZXhhbXBsZS1hcHAtbmFtZSIsInRva2VuIjoiZWM3OGE1YjYtYTNmNy0yODlhLWY0NGQtOWYzMTZlMzhkMzY5IiwiZXZlbnRzIjpbeyJpZCI6ImE2NjFjZTc2LTgzZWYtNDYwMS1iZTI3LTk2YzRhNTAyNDI1NiIsInN0YXJ0X3RpbWUiOjE2ODg3MTY0NjMuMzUxLCJyZXNvdXJjZSI6eyJuYW1lIjoibGFtYmRhLXRlc3QtTm9kZWpzU2FtcGxlVjJFcHNhZ29uLTdqN3ptUENTTUFseCIsInR5cGUiOiJsYW1iZGEiLCJvcGVyYXRpb24iOiJpbnZva2UiLCJtZXRhZGF0YSI6eyJhd3NfYWNjb3VudCI6IjIzMzI3MzgwOTE4MCIsImNvbGRfc3RhcnQiOiJ0cnVlIiwiZnVuY3Rpb25fdmVyc2lvbiI6IiRMQVRFU1QiLCJsb2dfZ3JvdXBfbmFtZSI6Ii9hd3MvbGFtYmRhL2xhbWJkYS10ZXN0LU5vZGVqc1NhbXBsZVYyRXBzYWdvbi03ajd6bVBDU01BbHgiLCJsb2dfc3RyZWFtX25hbWUiOiIyMDIzLzA3LzA3L1skTEFURVNUXTg3MTFmMDViMGJkOTRiZWI4OTQ3NGEyZjNlYTI5YTdhIiwibWVtb3J5IjoiMjU2IiwicmVnaW9uIjoiZXUtd2VzdC0xIiwicmV0dXJuX3ZhbHVlIjoie1wicmVzcG9uc2VcIjpcInRoaXMgaXNcIn0ifX0sIm9yaWdpbiI6InJ1bm5lciIsImR1cmF0aW9uIjoxLjYyNiwiZXJyb3JfY29kZSI6MCwiZXhjZXB0aW9uIjp7fX0seyJpZCI6InRyaWdnZXItM2EzOWNiOTYtNWQ0MC00NDk0LWIzMzItMzQ2YTA1ZmU5M2JhIiwic3RhcnRfdGltZSI6MTY4ODcxNjQ2My4zNTEsInJlc291cmNlIjp7Im5hbWUiOiJ0cmlnZ2VyLWxhbWJkYS10ZXN0LU5vZGVqc1NhbXBsZVYyRXBzYWdvbi03ajd6bVBDU01BbHgiLCJ0eXBlIjoianNvbiIsIm9wZXJhdGlvbiI6IkV2ZW50IiwibWV0YWRhdGEiOnsiZGF0YSI6eyJrZXkxIjoidmFsdWUxIiwia2V5MiI6InZhbHVlMiIsImtleTMiOiJ2YWx1ZTMifX19LCJvcmlnaW4iOiJ0cmlnZ2VyIiwiZHVyYXRpb24iOjAsImVycm9yX2NvZGUiOjAsImV4Y2VwdGlvbiI6e319LHsiaWQiOiJNNFFFWTY3UDkzTVNSSjVWIiwic3RhcnRfdGltZSI6MTY4ODcxNjQ2My4zNzMsInJlc291cmNlIjp7Im5hbWUiOiJsYW1iZGEtdGVzdC1pbmZyYSIsInR5cGUiOiJzMyIsIm9wZXJhdGlvbiI6ImdldE9iamVjdCIsIm1ldGFkYXRhIjp7ImV0YWciOiIzZGU4ZjhiMGRjOTRiOGMyMjMwZmFiOWVjMGJhMDUwNiIsImZpbGVfc2l6ZSI6IjIwIiwia2V5IjoidGVzdF9maWxlLnR4dCIsImxhc3RfbW9kaWZpZWQiOiJXZWQgTWF5IDEwIDIwMjMgMTM6MTk6MjEgR01UKzAwMDAgKENvb3JkaW5hdGVkIFVuaXZlcnNhbCBUaW1lKSIsInJlcXVlc3RfaWQiOiJNNFFFWTY3UDkzTVNSSjVWIiwicmV0cnlfYXR0ZW1wdHMiOiIwIiwic3RhdHVzX2NvZGUiOiIyMDAifX0sIm9yaWdpbiI6ImF3cy1zZGsiLCJkdXJhdGlvbiI6MC4yNTgsImVycm9yX2NvZGUiOjAsImV4Y2VwdGlvbiI6e319LHsiaWQiOiJhZjhmOWYyNC1lNDUyLTQxMmItYmE3OC05MDBlM2RjODZhMTIiLCJzdGFydF90aW1lIjoxNjg4NzE2NDYzLjY1LCJyZXNvdXJjZSI6eyJuYW1lIjoibGFtYmRhLXRlc3QtTm9kZWpzRmFpbEVwc2Fnb24tRU5UWjdWTzBkT3hYIiwidHlwZSI6ImxhbWJkYSIsIm9wZXJhdGlvbiI6Imludm9rZSIsIm1ldGFkYXRhIjp7InBheWxvYWQiOiIiLCJyZXF1ZXN0X2lkIjoiYWY4ZjlmMjQtZTQ1Mi00MTJiLWJhNzgtOTAwZTNkYzg2YTEyIiwicmV0cnlfYXR0ZW1wdHMiOiIwIiwic3RhdHVzX2NvZGUiOiIyMDAifX0sIm9yaWdpbiI6ImF3cy1zZGsiLCJkdXJhdGlvbiI6MS4wMjIsImVycm9yX2NvZGUiOjAsImV4Y2VwdGlvbiI6e319LHsiaWQiOiI0RTdQRlFTTkc3VkRDRUVOVkQxT09POFJUTlZWNEtRTlNPNUFFTVZKRjY2UTlBU1VBQUpHIiwic3RhcnRfdGltZSI6MTY4ODcxNjQ2NC42NzIsInJlc291cmNlIjp7Im5hbWUiOiJsYW1iZGEtdGVzdCIsInR5cGUiOiJkeW5hbW9kYiIsIm9wZXJhdGlvbiI6ImJhdGNoV3JpdGVJdGVtIiwibWV0YWRhdGEiOnsiQWRkZWQgSXRlbXMiOiJbe1widGVzdFwiOntcIlNcIjpcInRlc3RfdmFsdWVcIn19XSIsInJlcXVlc3RfaWQiOiI0RTdQRlFTTkc3VkRDRUVOVkQxT09POFJUTlZWNEtRTlNPNUFFTVZKRjY2UTlBU1VBQUpHIiwicmV0cnlfYXR0ZW1wdHMiOiIwIiwic3RhdHVzX2NvZGUiOiIyMDAiLCJ1bnByb2Nlc3NlZEl0ZW1zX2NvdW50IjowfX0sIm9yaWdpbiI6ImF3cy1zZGsiLCJkdXJhdGlvbiI6MC4wOCwiZXJyb3JfY29kZSI6MCwiZXhjZXB0aW9uIjp7fX0seyJpZCI6ImZmYWJmYWVlLWIxZTgtNTE5OC04ZTgyLWE3Y2Y3NzE4YzczYyIsInN0YXJ0X3RpbWUiOjE2ODg3MTY0NjQuNzUzLCJyZXNvdXJjZSI6eyJuYW1lIjoibGFtYmRhLXRlc3QtU3FzVGFyZ2V0LW0xaU1PSDl2cDIxQSIsInR5cGUiOiJzcXMiLCJvcGVyYXRpb24iOiJzZW5kTWVzc2FnZSIsIm1ldGFkYXRhIjp7Ik1ENSBPZiBNZXNzYWdlIEJvZHkiOiI4MmRmYTU1NDllYmM5YWZjMTY4ZWI3OTMxZWJlY2U1ZiIsIk1lc3NhZ2UgQm9keSI6IlRlc3QgbWVzc2FnZSIsIk1lc3NhZ2UgSUQiOiJmYTllZmUzOC1kZjFhLTQ2NTMtYWIzOC1lYzI1MzA5YTZjMzMiLCJyZXF1ZXN0X2lkIjoiZmZhYmZhZWUtYjFlOC01MTk4LThlODItYTdjZjc3MThjNzNjIiwicmV0cnlfYXR0ZW1wdHMiOiIwIiwic3RhdHVzX2NvZGUiOiIyMDAifX0sIm9yaWdpbiI6ImF3cy1zZGsiLCJkdXJhdGlvbiI6MC4wODcsImVycm9yX2NvZGUiOjAsImV4Y2VwdGlvbiI6e319LHsiaWQiOiI0YzA0YTIwYS03NDQ3LTRkODAtYTg3OS1iZGJkYTMzOGFlY2EiLCJzdGFydF90aW1lIjoxNjg4NzE2NDY0Ljg3MywicmVzb3VyY2UiOnsibmFtZSI6Ik5vZGVqc0ZhaWxFcHNhZ29uU3RhdGVNYWNoaW5lLW4yWEtnTzg3QUZGZCIsInR5cGUiOiJzdGVwZnVuY3Rpb25zIiwib3BlcmF0aW9uIjoic3RhcnRFeGVjdXRpb24iLCJtZXRhZGF0YSI6eyJFeGVjdXRpb24gQVJOIjoiYXJuOmF3czpzdGF0ZXM6ZXUtd2VzdC0xOjIzMzI3MzgwOTE4MDpleHByZXNzOk5vZGVqc0ZhaWxFcHNhZ29uU3RhdGVNYWNoaW5lLW4yWEtnTzg3QUZGZDo0ZWUwMDNjMy02N2QyLTRlODgtYWI2Yy00OTA1OWRlZTVkNjI6Mzc2NmZmMWYtZDA1NS00YzQ5LWJlYjEtOTEzOWFlZGQwZjE3IiwiRXhlY3V0aW9uIE5hbWUiOiJ1bmRlZmluZWQiLCJJbnB1dCI6Int9IiwiU3RhcnQgRGF0ZSI6IkZyaSBKdWwgMDcgMjAyMyAwNzo1NDoyNCBHTVQrMDAwMCAoQ29vcmRpbmF0ZWQgVW5pdmVyc2FsIFRpbWUpIiwiU3RhdGUgTWFjaGluZSBBUk4iOiJhcm46YXdzOnN0YXRlczpldS13ZXN0LTE6MjMzMjczODA5MTgwOnN0YXRlTWFjaGluZTpOb2RlanNGYWlsRXBzYWdvblN0YXRlTWFjaGluZS1uMlhLZ084N0FGRmQiLCJyZXF1ZXN0X2lkIjoiNGMwNGEyMGEtNzQ0Ny00ZDgwLWE4NzktYmRiZGEzMzhhZWNhIiwicmV0cnlfYXR0ZW1wdHMiOiIwIiwic3RhdHVzX2NvZGUiOiIyMDAifX0sIm9yaWdpbiI6ImF3cy1zZGsiLCJkdXJhdGlvbiI6MC4xMDQsImVycm9yX2NvZGUiOjAsImV4Y2VwdGlvbiI6e319XSwiZXhjZXB0aW9ucyI6W10sInZlcnNpb24iOiIxLjEyMy4zIiwicGxhdGZvcm0iOiJub2RlIDE0LjIxLjMifQ==\n".to_owned()),
        }
    );

    processor.process_telemetry(platform_runtime_done_event());

    assert_eq!(processor.output.epsagon_traces_buffer.len(), 1);
}

#[test]
fn spans_and_metrics_can_be_disabled() {
    let mut processor = TelemetryProcessorSetup::new(TelemetryProcessorConfig {
        platform_metrics_mode: PlatformMetricsMode::Disabled,
        tracing_mode: TracingMode::Disabled,
        ..default_config()
    });

    processor.process_telemetry(platform_start_event());
    processor.process_telemetry(function_log_event());
    processor.process_telemetry(platform_runtime_done_event());

    // This happens earliest during next invocation
    processor.process_telemetry(platform_report_event());

    assert_eq!(processor.output.spans_buffer.len(), 0);
    assert_eq!(processor.output.logs_buffer.len(), 4); // we expect the platform start log, function log and runtime done log, report log
    // no metrics where produced
    assert!(processor.metrics_state.make_metrics_report().is_empty())
}

#[test]
fn attributes_are_excluded() {
    let mut processor = TelemetryProcessorSetup::new(TelemetryProcessorConfig {
        tracing_mode: TracingMode::OtelInstrumentation,
        excluded_span_attributes: AttributeExclusionMode::Predefined,
        ..default_config()
    });

    processor.process_telemetry(platform_start_event());

    let resource_spans = vec![ResourceSpans {
        scope_spans: vec![ScopeSpans {
            spans: vec![
                Span {
                    trace_id: test_trace_id(),
                    span_id: vec![0, 0, 0, 0, 0, 0, 0, 1],
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 0],
                    name: "trigger".to_owned(),
                    kind: (SpanKind::Server as i32),
                    attributes: vec![
                        string_attribute("faas.trigger", "aaa"),
                        string_attribute("http.request.header.my-header", "value"), // this attribute is not matched by the exclude regex and should be kept
                        string_attribute("http.request.header.x-api-key", "Sensitive! Don't leak!"), // should be dropped in AttributeExclusionMode::Predefined
                        string_attribute(
                            "http.request.header.authorization",
                            "Sensitive! Don't leak!",
                        ), // should be dropped in AttributeExclusionMode::Predefined
                    ],
                    ..Span::default()
                },
                Span {
                    trace_id: test_trace_id(),
                    span_id: vec![0, 0, 0, 0, 0, 0, 0, 2],
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 1],
                    name: "invocation".to_owned(),
                    kind: (SpanKind::Server as i32),
                    attributes: vec![
                        string_attribute("http.request.header.my-header", "value"), // this attribute is not matched by the exclude regex and should be kept
                        string_attribute("http.request.header.x-api-key", "Sensitive! Don't leak!"), // should be dropped in AttributeExclusionMode::Predefined
                        string_attribute(
                            "http.request.header.authorization",
                            "Sensitive! Don't leak!",
                        ), // should be dropped in AttributeExclusionMode::Predefined
                    ],
                    ..Span::default()
                },
                Span {
                    trace_id: test_trace_id(),
                    span_id: vec![0, 0, 0, 0, 0, 0, 0, 3],
                    parent_span_id: vec![0, 0, 0, 0, 0, 0, 0, 2],
                    name: "internal".to_owned(),
                    kind: (SpanKind::Internal as i32),
                    attributes: vec![
                        string_attribute("http.request.header.my-header", "value"), // this attribute is not matched by the exclude regex and should be kept
                        string_attribute("http.request.header.x-api-key", "Sensitive! Don't leak!"), // should be dropped in AttributeExclusionMode::Predefined
                        string_attribute(
                            "http.request.header.authorization",
                            "Sensitive! Don't leak!",
                        ), // should be dropped in AttributeExclusionMode::Predefined
                    ],
                    ..Span::default()
                },
            ],
            ..ScopeSpans::default()
        }],
        ..ResourceSpans::default()
    }];
    processor.process_function_spans(resource_spans);

    processor.process_telemetry(platform_runtime_done_event());

    let spans = flatten_scopes(processor.output.take_function_spans());

    let trigger_span = assert_span(&spans, vec![0, 0, 0, 0, 0, 0, 0, 1], "trigger");
    assert_eq!(trigger_span.attributes.len(), 2);
    assert_attribute(trigger_span, "faas.trigger", "aaa");
    assert_attribute(trigger_span, "http.request.header.my-header", "value");

    let invocation_span = assert_span(&spans, vec![0, 0, 0, 0, 0, 0, 0, 2], "invocation");
    assert_eq!(invocation_span.attributes.len(), 1);
    assert_attribute(invocation_span, "http.request.header.my-header", "value");

    let internal_span = assert_span(&spans, vec![0, 0, 0, 0, 0, 0, 0, 3], "internal");
    assert_eq!(internal_span.attributes.len(), 1);
    assert_attribute(internal_span, "http.request.header.my-header", "value");
}

#[test]
fn default_metadata_has_execution_but_no_invocation() {
    let mut processor = TelemetryProcessorSetup::new(default_config());

    processor.process_telemetry(platform_start_event());
    processor.process_telemetry(platform_runtime_done_event());

    let logs = take(&mut processor.output.logs_buffer);
    assert_eq!(logs.len(), 2);
    let metadata = get_value(logs[0].body.as_ref().unwrap(), "cx_metadata").unwrap();
    assert_eq!(
        get_string_value(metadata, "span_id"),
        Some("0000000000000000")
    );
    assert_eq!(
        get_string_value(metadata, "trace_id"),
        Some("00000000000000000000000000000000")
    );
    assert_eq!(get_string_value(metadata, "faas_execution"), Some("0000"));
    assert_eq!(get_string_value(metadata, "faas_invocation_id"), None);
}

#[test]
fn logs_metadata_can_be_customized() {
    let mut processor = TelemetryProcessorSetup::new(TelemetryProcessorConfig {
        logs_metadata_mode: LogsMetadataMode::Enabled(LogMetadataConfig {
            include_trace_ref: false,
            include_execution: false,
            include_invocation_id: true,
        }),
        ..default_config()
    });

    processor.process_telemetry(platform_start_event());
    processor.process_telemetry(platform_runtime_done_event());

    let logs = take(&mut processor.output.logs_buffer);
    assert_eq!(logs.len(), 2);
    let metadata = get_value(logs[0].body.as_ref().unwrap(), "cx_metadata").unwrap();
    assert_eq!(get_string_value(metadata, "span_id"), None);
    assert_eq!(get_string_value(metadata, "trace_id"), None);
    assert_eq!(get_string_value(metadata, "faas_execution"), None);
    assert_eq!(
        get_string_value(metadata, "faas_invocation_id"),
        Some("0000")
    );
}

#[test]
fn metadata_can_be_disabled() {
    let mut processor = TelemetryProcessorSetup::new(TelemetryProcessorConfig {
        logs_metadata_mode: LogsMetadataMode::Disabled,
        ..default_config()
    });

    processor.process_telemetry(platform_start_event());
    processor.process_telemetry(platform_runtime_done_event());

    let logs = take(&mut processor.output.logs_buffer);
    assert_eq!(logs.len(), 2);
    assert_eq!(
        get_value(logs[0].body.as_ref().unwrap(), "cx_metadata"),
        None
    );
}
