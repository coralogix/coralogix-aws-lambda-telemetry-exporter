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

use super::span_id_to_string;
use super::span_processor;
use crate::config::app_config::TelemetryProcessorConfig;
use crate::proto::opentelemetry::proto::common::v1::KeyValue;
use crate::proto::opentelemetry::proto::trace::v1 as otel_trace;
use crate::proto::opentelemetry::proto::trace::v1::Span;
use crate::proto::opentelemetry::proto::trace::v1::span::SpanKind;
use crate::telemetry::xray_trace_context::XRayTraceContext;
use crate::telemetry::*;
use crate::telemetry::{
    DegradedProcessingState, InvocationContext, InvocationProcessingState, f64_attribute,
    string_attribute,
};
use lambda_extension::Status;
use std::mem::swap;
use tracing::debug;
use tracing::info;
use tracing::{trace, warn};

pub(super) fn make_degraded_spans_after_init(
    state: &DegradedProcessingState,
) -> Vec<otel_trace::Span> {
    let init_span = match (
        state.init_runtime_done_data.as_ref(),
        state.init_report_data.as_ref(),
    ) {
        (Some(done), Some(report)) => {
            let init_span_ref = state.current_span_ref();

            // We rely on report.metrics.duration_ms to calculate the start time without requiring initStart event which in practice is often lost because it is received by the previous instance before it crashes.
            let end_time_unix_nano = report.time.unix_timestamp_nanos() as u64;
            let start_time_unix_nano =
                end_time_unix_nano - (report.metrics.duration_ms * 1_000_000.0) as u64;

            Some(otel_trace::Span {
                trace_id: init_span_ref.trace_id,
                span_id: init_span_ref.span_id,
                trace_state: "".to_owned(),
                parent_span_id: Vec::new(),
                name: format!("{} init", state.function_context.arn.function_name),
                kind: SpanKind::Internal as i32,
                start_time_unix_nano,
                end_time_unix_nano,
                attributes: Vec::new(),
                dropped_attributes_count: 0,
                events: Vec::new(),
                dropped_events_count: 0,
                links: Vec::new(),
                dropped_links_count: 0,
                status: Some(translate_status(
                    done.status.clone(),
                    done.error_type.as_deref(),
                )),
            })
        }
        _ => None,
    };

    let mut spans = Vec::new();
    if let Some(init_span) = init_span {
        spans.push(init_span);
    }
    spans
}

pub(super) fn make_spans_after_invocation_in_otel_mode(
    state: &InvocationProcessingState,
) -> Vec<otel_trace::Span> {
    let trace_id = state.current_trace_id();

    let init_parent_span_id = state
        .invocation_span()
        .map_or_else(Vec::new, |s| s.parent_span_id.clone());
    let init_span = match (
        state.init_span_id.as_ref(),
        state.init_start_data.as_ref(),
        state.init_runtime_done_data.as_ref(),
        state.init_report_data.as_ref(),
    ) {
        (Some(init_span_id), Some(start), Some(done), Some(report)) => Some(otel_trace::Span {
            trace_id: trace_id.clone(),
            span_id: init_span_id.clone(),
            trace_state: "".to_owned(),
            parent_span_id: init_parent_span_id,
            name: format!("{} init", state.function_context.arn.function_name),
            kind: SpanKind::Internal as i32,
            start_time_unix_nano: start.time.unix_timestamp_nanos() as u64,
            end_time_unix_nano: report.time.unix_timestamp_nanos() as u64,
            attributes: make_span_attributes(&state.invocation_context),
            dropped_attributes_count: 0,
            events: Vec::new(),
            dropped_events_count: 0,
            links: Vec::new(),
            dropped_links_count: 0,
            status: Some(translate_status(
                done.status.clone(),
                done.error_type.as_deref(),
            )),
        }),
        (_, None, None, None) => None, // No init happened. This isn't the first invocation handled by this lambda execution environment.
        (init_span_id, start, done, report) => {
            warn!(
                "Received an incomplete set of init telemetry events: {} {} {} {:?}",
                start.is_some(),
                done.is_some(),
                report.is_some(),
                init_span_id.map(span_id_to_string),
            );
            None
        }
    };

    let mut spans = Vec::new();
    if let Some(init_span) = init_span {
        spans.push(init_span);
    }
    spans
}

pub(super) fn make_spans_after_invocation(
    state: &InvocationProcessingState,
) -> Vec<otel_trace::Span> {
    let invocation_span_ref = state.current_span_ref();
    let trace_id = invocation_span_ref.trace_id.clone();

    let init_span = match (
        state.init_span_id.as_ref(),
        state.init_start_data.as_ref(),
        state.init_runtime_done_data.as_ref(),
        state.init_report_data.as_ref(),
    ) {
        (Some(init_span_id), Some(start), Some(done), Some(report)) => Some(otel_trace::Span {
            trace_id,
            span_id: init_span_id.clone(),
            trace_state: "".to_owned(),
            parent_span_id: state.root_span_id.clone(),
            name: format!("{} init", state.function_context.arn.function_name),
            kind: SpanKind::Internal as i32,
            start_time_unix_nano: start.time.unix_timestamp_nanos() as u64,
            end_time_unix_nano: report.time.unix_timestamp_nanos() as u64,
            attributes: make_span_attributes(&state.invocation_context),
            dropped_attributes_count: 0,
            events: Vec::new(),
            dropped_events_count: 0,
            links: Vec::new(),
            dropped_links_count: 0,
            status: Some(translate_status(
                done.status.clone(),
                done.error_type.as_deref(),
            )),
        }),
        (None, None, None, None) => None, // No init happened. This isn't the first invocation handled by this lambda execution environment.
        (init_span_id, start, done, report) => {
            warn!(
                "Received an incomplete set of init telemetry events: {} {} {} {}",
                init_span_id.is_some(),
                start.is_some(),
                done.is_some(),
                report.is_some()
            );
            None
        }
    };

    let invocation_span = match (state.start_data.as_ref(), state.runtime_done_data.as_ref()) {
        (Some(start), Some(done)) => {
            let mut attributes = make_span_attributes(&state.invocation_context);

            let response_latency = done
                .spans
                .iter()
                .find(|span| span.name == "responseLatency")
                .map(|x| x.duration_ms);

            if let Some(response_latency) = response_latency {
                attributes.push(f64_attribute("response.latency_ms", response_latency))
            }

            let response_duration = done
                .spans
                .iter()
                .find(|span| span.name == "responseDuration")
                .map(|x| x.duration_ms);

            if let Some(response_duration) = response_duration {
                attributes.push(f64_attribute("response.duration_ms", response_duration))
            }

            let invocation_status =
                translate_status(done.status.clone(), done.error_type.as_deref());

            Some(otel_trace::Span {
                trace_id: invocation_span_ref.trace_id,
                span_id: invocation_span_ref.span_id,
                trace_state: "".to_owned(), // TODO AWS's doc says something about using "Sampled" here, but I'm not sure what's the point and if it makes any sense in the context of pushing this trace to Coralogix
                parent_span_id: state.root_span_id.clone(), // We ignore the parent id provided by AWS, because we know nothing about that span. Instead we synthesize our own parent above. Is this the right approach? I don't know. In case we would like to use the parent id: trace_context.map_or_else(Vec::new, |x| x.parent),
                name: format!("{} invocation", state.function_context.arn.function_name),
                kind: SpanKind::Internal as i32,
                start_time_unix_nano: start.time.unix_timestamp_nanos() as u64,
                end_time_unix_nano: done.time.unix_timestamp_nanos() as u64,
                attributes,
                dropped_attributes_count: 0,
                events: Vec::new(),
                dropped_events_count: 0,
                links: Vec::new(),
                dropped_links_count: 0,
                status: Some(invocation_status),
            })
        }
        (start, done) => {
            warn!(
                "Received an incomplete set of invocation telemetry events: {} {}",
                start.is_some(),
                done.is_some(),
            );
            None
        }
    };

    trace!(
        "Invocation span {:?} is child of {:?}, but it could be {:?}",
        invocation_span
            .as_ref()
            .map(|x| span_id_to_string(&x.span_id)),
        invocation_span
            .as_ref()
            .map(|x| span_id_to_string(&x.parent_span_id.clone())),
        state
            .start_data
            .as_ref()
            .and_then(|x| x.tracing.as_ref().map(|x| x.value.as_str()))
            .and_then(|x| XRayTraceContext::try_from(x).ok())
            .map(|x| span_id_to_string(&x.parent)),
    );

    let mut spans = Vec::new();
    if let Some(s) = init_span {
        spans.push(s)
    }
    if let Some(s) = invocation_span {
        spans.push(s)
    }
    spans
}

pub(super) fn make_spans_after_report(state: &InvocationProcessingState) -> Vec<otel_trace::Span> {
    let trace_id = state.current_trace_id();

    let context_span = match (
        state.start_data.as_ref(),
        state.runtime_done_data.as_ref(),
        state.report_data.as_ref(),
    ) {
        (Some(start), Some(done), Some(report)) => {
            let status = translate_status(
                report.status.clone(),
                report.error_type.as_deref().or(done.error_type.as_deref()),
            );

            let start_time_nano = state
                .init_start_data
                .as_ref()
                .map(|isd| isd.time.unix_timestamp_nanos() as u64)
                .or_else(|| {
                    state.init_report_data.as_ref().map(|ird| {
                        (ird.time.unix_timestamp_nanos()
                            - (ird.metrics.duration_ms * 1000000.0) as i128)
                            as u64
                    })
                })
                .unwrap_or_else(|| start.time.unix_timestamp_nanos() as u64);

            Some(otel_trace::Span {
                trace_id: trace_id.clone(),
                span_id: state.root_span_id.clone(),
                trace_state: "".to_owned(),
                parent_span_id: Vec::new(),
                name: format!(
                    "{} context",
                    &state.invocation_context.invoked_arn.function_name
                ),
                kind: SpanKind::Server as i32,
                start_time_unix_nano: start_time_nano,
                end_time_unix_nano: report.time.unix_timestamp_nanos() as u64,
                attributes: make_span_attributes(&state.invocation_context),
                dropped_attributes_count: 0,
                events: Vec::new(),
                dropped_events_count: 0,
                links: Vec::new(),
                dropped_links_count: 0,
                status: Some(status), // as far as I can tell, we receive the start / done / report even if the init failed so we can just use that status and it should be failed if init failed.
            })
        }
        (start, done, report) => {
            warn!(
                "Received an incomplete set of invocation telemetry events: {} {} {}",
                start.is_some(),
                done.is_some(),
                report.is_some()
            );
            None
        }
    };

    trace!(
        "Context span {:?} is child of {:?}",
        context_span
            .as_ref()
            .map(|x| span_id_to_string(&x.span_id.clone())),
        context_span
            .as_ref()
            .map(|x| span_id_to_string(&x.parent_span_id.clone())),
    );

    let mut spans = Vec::new();
    if let Some(s) = context_span {
        spans.push(s)
    }
    spans
}

fn make_span_attributes(context: &InvocationContext) -> Vec<KeyValue> {
    vec![
        // https://github.com/open-telemetry/opentelemetry-specification/blob/v1.17.0/specification/trace/semantic_conventions/faas.md?plain=1#L41
        // This has changed since and our implementation is out-of-date with the latest conventions https://github.com/open-telemetry/semantic-conventions/blob/v1.32.0/docs/faas/faas-spans.md?plain=1#L51
        string_attribute("faas.execution", &context.request_id),
    ]
}

pub(super) fn process_function_spans(
    state: &mut InvocationTracingState,
    mut function_spans: Vec<ResourceSpans>,
    config: &TelemetryProcessorConfig,
) -> Vec<ResourceSpans> {
    detect_language(state, &function_spans);
    process_spans(state, &mut function_spans, config);
    function_spans
}

fn detect_language(state: &mut InvocationTracingState, function_spans: &[ResourceSpans]) {
    if state.language.is_none() {
        state.language = function_spans
            .iter()
            .flat_map(|rs| &rs.resource)
            .find_map(|r| find_attribute(&r.attributes, LANGUAGE_ATTRIBUTE))
            .cloned();
    }
}

fn process_spans(
    state: &mut InvocationTracingState,
    resource_spans: &mut [ResourceSpans],
    config: &TelemetryProcessorConfig,
) {
    for rs in resource_spans.iter_mut() {
        for ss in rs.scope_spans.iter_mut() {
            let scope = ss.scope.as_ref();
            state.processed_function_spans += ss.spans.len();
            let mut spans = Vec::with_capacity(ss.spans.len());
            swap(&mut ss.spans, &mut spans);

            for span in spans {
                let (mut span, category) = process_and_categorize_span(span);
                // user-defined filters could break categorization if filtering happened first
                span_processor::filter_span_attributes(&mut span, config);

                let capturing_result = match category {
                    SpanCategorization::CxEarlyTrigger => {
                        try_capture_span(&mut state.cx_early_trigger_span, span, scope)
                    }
                    SpanCategorization::CxEarlyInvocation => {
                        try_capture_span(&mut state.cx_early_invocation_span, span, scope)
                    }
                    SpanCategorization::CxTrigger => {
                        try_capture_span(&mut state.cx_trigger_span, span, scope)
                    }
                    SpanCategorization::CxInvocation => {
                        try_capture_span(&mut state.cx_invocation_span, span, scope)
                    }
                    SpanCategorization::GenericInvocation => {
                        try_capture_span(&mut state.generic_invocation_span, span, scope)
                    }
                    SpanCategorization::CxWarmup => SpanCapturingResult::Discarded,
                    SpanCategorization::Other => SpanCapturingResult::NonCapturable(span),
                };
                match capturing_result {
                    SpanCapturingResult::Captured => (),
                    SpanCapturingResult::Discarded => (),
                    SpanCapturingResult::NonCapturable(span) => ss.spans.push(span),
                    SpanCapturingResult::Duplicate(span) => {
                        warn!("Detected duplicate {:?} span", category);
                        ss.spans.push(span)
                    }
                }
            }
        }
        rs.scope_spans.retain_mut(|ss| !ss.spans.is_empty())
    }
}

fn process_and_categorize_span(mut span: Span) -> (Span, SpanCategorization) {
    let mut span_role: Option<String> = None;
    let mut is_early_span = false;
    let mut extra_span_id: Option<String> = None;
    let mut extra_trace_id: Option<String> = None;
    let mut has_faas_trigger = false;

    let mut new_attributes = Vec::with_capacity(span.attributes.len());
    for attr in span.attributes {
        // remove and process cx internal attributes
        if attr.key == SPAN_ROLE_ATTRIBUTE {
            span_role = get_string_value(attr)
        } else if attr.key == SPAN_STATE_ATTRIBUTE {
            if get_string_value(attr).as_deref() == Some("early") {
                is_early_span = true;
            }
        } else if attr.key == SPAN_ID_ATTRIBUTE {
            extra_span_id = get_string_value(attr)
        } else if attr.key == TRACE_ID_ATTRIBUTE {
            extra_trace_id = get_string_value(attr)
        // keep other attributes
        } else if attr.key == FAAS_TRIGGER_ATTRIBUTE {
            has_faas_trigger = true;
            new_attributes.push(attr);
        } else {
            new_attributes.push(attr);
        }
    }
    span.attributes = new_attributes;

    if let Some(span_role) = span_role {
        if is_early_span {
            // extra_span_id/extra_trace_id overwrites the span_id/trace_id of the span.
            if let Some(extra_span_id) = extra_span_id {
                match hex::decode(&extra_span_id) {
                    Ok(span_id) => span.span_id = span_id,
                    Err(error) => warn!(?error, ?extra_span_id, "Failed to decode extra span id"),
                }
            }
            if let Some(extra_trace_id) = extra_trace_id {
                match hex::decode(&extra_trace_id) {
                    Ok(trace_id) => span.trace_id = trace_id,
                    Err(error) => warn!(?error, ?extra_trace_id, "Failed to decode extra trace id"),
                }
            }
            match span_role.as_str() {
                "trigger" => (span, SpanCategorization::CxEarlyTrigger),
                "invocation" => (span, SpanCategorization::CxEarlyInvocation),
                "warmup" => (span, SpanCategorization::CxWarmup),
                _ => (span, SpanCategorization::Other),
            }
        } else {
            match span_role.as_str() {
                "trigger" => (span, SpanCategorization::CxTrigger),
                "invocation" => (span, SpanCategorization::CxInvocation),
                "warmup" => (span, SpanCategorization::CxWarmup),
                _ => (span, SpanCategorization::Other),
            }
        }
    } else if (span.kind == (SpanKind::Server as i32) || span.kind == (SpanKind::Consumer as i32))
        && !has_faas_trigger
    {
        (span, SpanCategorization::GenericInvocation)
    } else {
        (span, SpanCategorization::Other)
    }
}

fn get_string_value(attribute: KeyValue) -> Option<String> {
    attribute.value.and_then(|v| v.value).and_then(|v| match v {
        Value::StringValue(s) => Some(s),
        _ => None,
    })
}

#[derive(Debug)]
enum SpanCategorization {
    // New versions of Coralogix OTEL auto-instrumentation emit spans marked with `cx.internal.*` attributes, that fall into these categories
    CxEarlyTrigger,
    CxEarlyInvocation,
    CxTrigger,
    CxInvocation,
    CxWarmup,
    // This is used in case of a non-Coralogix OTEL instrumentation. The instrumentation is expected to emit a single SERVER span (not counting spans with faas.trigger).
    GenericInvocation,
    // All other spans go here
    Other,
}

fn filter_span_attributes(span: &mut otel_trace::Span, config: &TelemetryProcessorConfig) {
    if let Some(regex) = config.excluded_span_attributes.regex() {
        span.attributes
            .retain(|attr| !regex.is_match(attr.key.as_str()));
    }
}

fn try_capture_span(
    storage: &mut Option<SpanWithScope>,
    span: Span,
    scope: Option<&InstrumentationScope>,
) -> SpanCapturingResult {
    if storage.is_none() {
        storage.replace(SpanWithScope {
            span,
            scope: scope.cloned(),
        });
        SpanCapturingResult::Captured
    } else {
        SpanCapturingResult::Duplicate(span)
    }
}

enum SpanCapturingResult {
    Captured,
    Discarded,
    NonCapturable(Span),
    Duplicate(Span),
}

pub(super) fn emit_trigger_and_invocation_spans(
    state: &mut InvocationProcessingState,
) -> Vec<ScopeSpans> {
    let done = if let Some(done) = state.runtime_done_data.as_ref() {
        done
    } else {
        warn!("Missing runtime done data!");
        return Vec::new();
    };

    let tracing_state: &mut InvocationTracingState = &mut state.tracing_state;

    let mut spans = Vec::new();

    // TODO currently we clone the spans here, because removing them from the state breaks the current_span_id() logic.

    // Emit trigger span
    if let Some(s) = tracing_state.cx_trigger_span.clone() {
        spans.push(s.into_scope_spans())
    } else if let Some(mut s) = tracing_state.cx_early_trigger_span.clone() {
        conclude_early_span_with_runtime_status(done, &mut s.span);
        spans.push(s.into_scope_spans())
    }

    // Emit invocation span
    if let Some(mut s) = tracing_state.cx_invocation_span.clone() {
        add_language_attribute_to_span(tracing_state, &mut s.span);
        spans.push(s.into_scope_spans())
    } else if let Some(mut s) = tracing_state.cx_early_invocation_span.clone() {
        conclude_early_span_with_runtime_status(done, &mut s.span);
        add_language_attribute_to_span(tracing_state, &mut s.span);
        spans.push(s.into_scope_spans())
    } else if let Some(mut s) = tracing_state.generic_invocation_span.clone() {
        add_language_attribute_to_span(tracing_state, &mut s.span);
        spans.push(s.into_scope_spans())
    } else {
        log_missing_invocation_span(tracing_state.processed_function_spans, &done.status)
    }

    if tracing_state.cx_invocation_span.is_some()
        || tracing_state.cx_early_invocation_span.is_some()
    {
        // We don't expect to have a generic_invocation_span when we have a cx_(early)_invocation_span
        if let Some(s) = tracing_state.generic_invocation_span.clone() {
            debug!(name = ?s.span.name, spanId = ?s.span.span_id, "Found unexpected invocation span (kind=SERVER)");
            // Emit it as a regular span
            spans.push(s.into_scope_spans())
        }
    }

    spans
}

fn conclude_early_span_with_runtime_status(done: &PlatformRuntimeDoneData, span: &mut Span) {
    span.status = Some(translate_status(
        done.status.clone(),
        done.error_type.as_deref(),
    ));
    span.end_time_unix_nano = done.time.unix_timestamp_nanos() as u64
}

fn add_language_attribute_to_span(state: &InvocationTracingState, span: &mut Span) {
    if let Some(language) = state.language.as_ref() {
        span.attributes.push(KeyValue {
            key: LANGUAGE_ATTRIBUTE.to_owned(),
            value: Some(AnyValue {
                value: Some(language.to_owned()),
            }),
        })
    }
}

fn log_missing_invocation_span(processed_function_spans: usize, status: &Status) {
    if processed_function_spans == 0 {
        if status == &Status::Success {
            info!(
                "No function spans have been received. This may indicate an issue with OTEL auto-instrumentation. If you're instrumenting the function by yourself using OTEL SDK please make sure that you flush spans before the lambda handler completes"
            );
        } else {
            debug!(
                ?status,
                "No function spans have been received because the runtime failed."
            );
        }
    } else {
        info!(
            "Found no invocation span, despite receiving {} function spans. This may indicate an issue with OTEL auto-instrumentation.",
            processed_function_spans,
        )
    }
}

fn translate_status(
    status: lambda_extension::Status,
    error_type: Option<&str>,
) -> otel_trace::Status {
    match status {
        // TODO consider adding OOM here
        lambda_extension::Status::Success => otel_trace::Status {
            message: "OK".to_owned(),
            code: otel_trace::status::StatusCode::Ok as i32,
        },
        lambda_extension::Status::Timeout => otel_trace::Status {
            message: error_type.unwrap_or("timeout").to_owned(),
            code: otel_trace::status::StatusCode::Error as i32,
        },
        lambda_extension::Status::Error => otel_trace::Status {
            message: error_type.unwrap_or("error").to_owned(),
            code: otel_trace::status::StatusCode::Error as i32,
        },
        lambda_extension::Status::Failure => otel_trace::Status {
            message: error_type.unwrap_or("failure").to_owned(),
            code: otel_trace::status::StatusCode::Error as i32,
        },
    }
}

const LANGUAGE_ATTRIBUTE: &str = "telemetry.sdk.language";
const FAAS_TRIGGER_ATTRIBUTE: &str = "faas.trigger";

// All "cx.internal.*" attributes are produced by our auto-instrumentation and stripped by the telemetry-exporter
// Used by instrumentation to explicitly mark the "trigger" and "invocation" spans
const SPAN_ROLE_ATTRIBUTE: &str = "cx.internal.span.role";
// Used by instrumentation to mark copies of spans that are delivered at the beginning of an invocation
const SPAN_STATE_ATTRIBUTE: &str = "cx.internal.span.state";

// These attributes are used to carry the real span_id and trace_id from the instrumentation to the telemetry-exporter. This is done because some SDK's (java) don't allow directly setting the IDs.
const SPAN_ID_ATTRIBUTE: &str = "cx.internal.span.id";
const TRACE_ID_ATTRIBUTE: &str = "cx.internal.trace.id";
