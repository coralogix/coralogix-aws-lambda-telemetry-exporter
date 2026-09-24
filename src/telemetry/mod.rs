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

use self::function_context::FunctionContext;
use self::lambda_function_arn::LambdaFunctionArn;
use self::xray_trace_context::XRayTraceContext;
use crate::config::app_config::{TraceSamplingMode, TracingMode};
use crate::proto::opentelemetry::proto::common::v1::any_value::Value;
use crate::proto::opentelemetry::proto::common::v1::{AnyValue, InstrumentationScope, KeyValue};
use crate::proto::opentelemetry::proto::trace::v1::ResourceSpans;
use crate::proto::opentelemetry::proto::trace::v1::{ScopeSpans, Span};
use lambda_extension::{InitPhase, InitReportMetrics, InitType, LambdaTelemetry};
use lambda_extension::{ReportMetrics, RuntimeDoneMetrics, TraceContext};
use std::iter::repeat_with;
use std::sync::Arc;
use time::OffsetDateTime;
use tracing::error;

pub mod function_context;
pub mod function_tags_provider;
pub mod lambda_function_arn;
mod otel_metrics;
mod otlp_wrappers;
mod processor;
mod sending_supervisor;
pub mod telemetry_sender;
pub mod telemetry_service;
mod xray_trace_context;

#[cfg(test)]
mod tests;

trait ProcessingState {
    fn get_runtime_done_data(&self) -> Option<&PlatformRuntimeDoneData>;
    fn get_report_data(&self) -> Option<&PlatformReportData>;
    fn current_span_ref(&self) -> SpanRef;
    fn request_id(&self) -> Option<String>;
    fn function_context(&self) -> &FunctionContext;
    fn runtime_done_data(&self) -> &Option<PlatformRuntimeDoneData>;
    fn last_log_state_mut(&mut self) -> &mut LastLogState;
}

#[derive(Debug)]
pub struct InvocationProcessingState {
    function_context: Arc<FunctionContext>,
    invocation_context: InvocationContext,
    root_span_id: Vec<u8>,

    init_span_id: Option<Vec<u8>>,
    init_start_data: Option<PlatformInitStartData>,
    init_runtime_done_data: Option<PlatformInitRuntimeDoneData>,
    init_report_data: Option<PlatformInitReportData>,

    start_data: Option<PlatformStartData>,
    runtime_done_data: Option<PlatformRuntimeDoneData>,
    report_data: Option<PlatformReportData>,

    delaying_logs: bool,
    logs: Vec<RawFunctionLog>,
    tracing_state: InvocationTracingState,

    last_log_state: LastLogState,
}

impl InvocationProcessingState {
    fn new(
        function_context: Arc<FunctionContext>,
        invocation_context: InvocationContext,
        trace_sampling_mode: TraceSamplingMode,
        tracing_mode: TracingMode,
    ) -> InvocationProcessingState {
        let tracing_state = InvocationTracingState::new(trace_sampling_mode, &invocation_context);
        let delaying_logs = matches!(tracing_mode, TracingMode::OtelInstrumentation);
        InvocationProcessingState {
            function_context,
            invocation_context,
            root_span_id: random_span_id(),
            init_span_id: None,
            init_start_data: None,
            init_runtime_done_data: None,
            init_report_data: None,
            start_data: None,
            runtime_done_data: None,
            report_data: None,
            delaying_logs,
            logs: Vec::new(),
            tracing_state,
            last_log_state: LastLogState::default(),
        }
    }

    fn current_trace_id(&self) -> &Vec<u8> {
        self.invocation_span()
            .map(|s| &s.trace_id)
            .unwrap_or(&self.invocation_context.trace_id)
    }

    fn current_span_id(&self) -> &Vec<u8> {
        if self.init_start_data.is_some() && self.init_report_data.is_none() {
            self.init_span_id
                .as_ref()
                .unwrap_or(&self.invocation_context.invocation_span_id)
        } else {
            self.invocation_span()
                .map(|s| &s.span_id)
                .unwrap_or(&self.invocation_context.invocation_span_id)
        }
    }

    fn invocation_span(&self) -> Option<&Span> {
        self.tracing_state
            .cx_invocation_span
            .as_ref()
            .or(self.tracing_state.cx_early_invocation_span.as_ref())
            .or(self.tracing_state.generic_invocation_span.as_ref())
            .map(|s| &s.span)
    }
}

impl ProcessingState for InvocationProcessingState {
    fn get_runtime_done_data(&self) -> Option<&PlatformRuntimeDoneData> {
        self.runtime_done_data.as_ref()
    }

    fn get_report_data(&self) -> Option<&PlatformReportData> {
        self.report_data.as_ref()
    }

    fn current_span_ref(&self) -> SpanRef {
        SpanRef {
            trace_id: self.current_trace_id().clone(),
            span_id: self.current_span_id().clone(),
        }
    }

    fn request_id(&self) -> Option<String> {
        Some(self.invocation_context.request_id.clone())
    }

    fn function_context(&self) -> &FunctionContext {
        &self.function_context
    }

    fn runtime_done_data(&self) -> &Option<PlatformRuntimeDoneData> {
        &self.runtime_done_data
    }

    fn last_log_state_mut(&mut self) -> &mut LastLogState {
        &mut self.last_log_state
    }
}

#[derive(Debug)]
pub struct DegradedProcessingState {
    function_context: Arc<FunctionContext>,

    trace_id: Vec<u8>,
    span_id: Vec<u8>,

    init_runtime_done_data: Option<PlatformInitRuntimeDoneData>,
    init_report_data: Option<PlatformInitReportData>,
    runtime_done_data: Option<PlatformRuntimeDoneData>,
    report_data: Option<PlatformReportData>,

    last_log_state: LastLogState,
}

impl DegradedProcessingState {
    fn new(function_context: Arc<FunctionContext>) -> DegradedProcessingState {
        DegradedProcessingState {
            function_context,
            // Being just random ids, these are not useful for correlation with anything external,
            // But giving all the logs a common pair of trace_id and span_id makes it easy to understand which logs belong to one sequence of events and which are independent.
            trace_id: random_trace_id(),
            span_id: random_span_id(),

            init_runtime_done_data: None,
            init_report_data: None,
            runtime_done_data: None,
            report_data: None,

            last_log_state: Default::default(),
        }
    }
}

impl ProcessingState for DegradedProcessingState {
    fn get_runtime_done_data(&self) -> Option<&PlatformRuntimeDoneData> {
        self.runtime_done_data.as_ref()
    }

    fn get_report_data(&self) -> Option<&PlatformReportData> {
        self.report_data.as_ref()
    }

    fn current_span_ref(&self) -> SpanRef {
        SpanRef {
            trace_id: self.trace_id.clone(),
            span_id: self.span_id.clone(),
        }
    }

    fn request_id(&self) -> Option<String> {
        None
    }

    fn function_context(&self) -> &FunctionContext {
        &self.function_context
    }

    fn runtime_done_data(&self) -> &Option<PlatformRuntimeDoneData> {
        &self.runtime_done_data
    }

    fn last_log_state_mut(&mut self) -> &mut LastLogState {
        &mut self.last_log_state
    }
}

#[derive(Debug)]
struct LastLogState {
    last_log_timestamp: OffsetDateTime,
    last_log_subindex: u32,
}

impl Default for LastLogState {
    fn default() -> Self {
        Self {
            last_log_timestamp: OffsetDateTime::UNIX_EPOCH,
            last_log_subindex: 0,
        }
    }
}

#[derive(Debug)]
struct InvocationTracingState {
    should_produce_spans: bool,
    processed_function_spans: usize,
    language: Option<Value>,
    cx_early_trigger_span: Option<SpanWithScope>,
    cx_trigger_span: Option<SpanWithScope>,
    cx_early_invocation_span: Option<SpanWithScope>,
    cx_invocation_span: Option<SpanWithScope>,
    generic_invocation_span: Option<SpanWithScope>,
}

impl InvocationTracingState {
    fn new(
        trace_sampling_mode: TraceSamplingMode,
        invocation_context: &InvocationContext,
    ) -> InvocationTracingState {
        let should_produce_spans = match trace_sampling_mode {
            TraceSamplingMode::All => true,
            TraceSamplingMode::FollowXray => invocation_context.sampled,
        };

        InvocationTracingState {
            should_produce_spans,
            processed_function_spans: 0,
            language: None,
            cx_early_trigger_span: None,
            cx_trigger_span: None,
            cx_early_invocation_span: None,
            cx_invocation_span: None,
            generic_invocation_span: None,
        }
    }
}

#[derive(Debug, Clone)]
struct SpanWithScope {
    span: Span,
    scope: Option<InstrumentationScope>,
}

impl SpanWithScope {
    fn into_scope_spans(self) -> ScopeSpans {
        ScopeSpans {
            scope: self.scope,
            spans: vec![self.span],
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone)]
struct RawFunctionLog {
    log_text: String,
    time: OffsetDateTime,
}

#[derive(Debug, Clone)]
struct InvocationContext {
    // the invoked ARN may be without version, with version, or with version alias
    invoked_arn: LambdaFunctionArn,
    request_id: String,
    trace_id: Vec<u8>,
    invocation_span_id: Vec<u8>,
    sampled: bool,
}

impl TryFrom<&InvokeEventDataWithSpans> for InvocationContext {
    type Error = lambda_extension::Error;

    fn try_from(invoke_event: &InvokeEventDataWithSpans) -> Result<Self, Self::Error> {
        let xray_trace_context =
            match XRayTraceContext::try_from(invoke_event.tracing.value.as_str()) {
                Ok(xray) => xray,
                Err(err) => {
                    error!(
                        err,
                        "Failed to parse XRayTraceContext '{}'",
                        invoke_event.tracing.value.as_str()
                    );
                    XRayTraceContext {
                        root: random_trace_id(),
                        parent: random_span_id(),
                        sampled: true,
                    }
                }
            };
        Ok(InvocationContext {
            invoked_arn: invoke_event
                .invoked_function_arn
                .parse::<LambdaFunctionArn>()?,
            request_id: invoke_event.request_id.clone(),
            trace_id: xray_trace_context.root,
            invocation_span_id: xray_trace_context.parent,
            sampled: xray_trace_context.sampled,
        })
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct InvokeEventDataWithSpans {
    deadline_ms: u64,
    request_id: String,
    invoked_function_arn: String,
    tracing: InvokeTraceContext,
    spans: Vec<ResourceSpans>,
    function_context: Arc<FunctionContext>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct InvokeTraceContext {
    r#type: String,
    value: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PlatformInitStartData {
    time: OffsetDateTime,
    initialization_type: InitType,
    phase: InitPhase,
    runtime_version: Option<String>,
    runtime_version_arn: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PlatformInitRuntimeDoneData {
    time: OffsetDateTime,
    initialization_type: InitType,
    phase: Option<InitPhase>,
    status: lambda_extension::Status,
    error_type: Option<String>,
    spans: Vec<lambda_extension::Span>, // I have found no case where this would contain any spans
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PlatformInitReportData {
    time: OffsetDateTime,
    initialization_type: InitType,
    phase: InitPhase,
    metrics: InitReportMetrics,
    spans: Vec<lambda_extension::Span>, // I have found no case where this would contain any spans
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PlatformStartData {
    time: OffsetDateTime,
    request_id: String,
    version: Option<String>,
    tracing: Option<TraceContext>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PlatformRuntimeDoneData {
    time: OffsetDateTime,
    request_id: String,
    status: lambda_extension::Status,
    error_type: Option<String>,
    metrics: Option<RuntimeDoneMetrics>,
    spans: Vec<lambda_extension::Span>,
    tracing: Option<TraceContext>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PlatformReportData {
    time: OffsetDateTime,
    request_id: String,
    status: lambda_extension::Status,
    error_type: Option<String>,
    metrics: ReportMetrics,
    spans: Vec<lambda_extension::Span>, // I have found no case where this would contain any spans
    tracing: Option<TraceContext>,
}

fn random_trace_id() -> Vec<u8> {
    repeat_with(|| fastrand::u8(..)).take(16).collect()
}

fn random_span_id() -> Vec<u8> {
    repeat_with(|| fastrand::u8(..)).take(8).collect()
}

fn string_attribute(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(Value::StringValue(value.to_owned())),
        }),
    }
}

fn f64_attribute(key: &str, value: f64) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(Value::DoubleValue(value)),
        }),
    }
}

fn find_attribute<'a>(attributes: &'a [KeyValue], key: &str) -> Option<&'a Value> {
    attributes
        .iter()
        .find(|kv| kv.key == key)
        .and_then(|kv| kv.value.as_ref())
        .and_then(|av| av.value.as_ref())
}

fn now_nanos() -> u64 {
    OffsetDateTime::now_utc().unix_timestamp_nanos() as u64
}

struct SpanRef {
    pub trace_id: Vec<u8>,
    pub span_id: Vec<u8>,
}

fn event_timestamp(event: &LambdaTelemetry) -> Result<OffsetDateTime, time::error::ComponentRange> {
    // Resolution of the timestamp provided by AWS is milliseconds anyway
    OffsetDateTime::from_unix_timestamp_nanos((event.time.timestamp_millis() as i128) * 1_000_000)
}
