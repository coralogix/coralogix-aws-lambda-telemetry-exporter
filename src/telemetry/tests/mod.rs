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
use crate::config::app_config::TelemetryReportingStrategy::*;
use crate::config::app_config::TracingMode::*;
use crate::config::app_config::*;
use crate::coralogix;
use crate::coralogix::OtlpExportResponse;
use crate::coralogix::OtlpSender;
use crate::coralogix::coralogix_sender::CoralogixTelemetrySender;
use crate::coralogix::coralogix_sender::NoopEpsagonTracesTelemetrySender;
use crate::coralogix::coralogix_sender::OtlpPillarTelemetrySender;
use crate::proto::opentelemetry::proto::collector::trace::v1::ExportTraceServiceRequest;
use crate::proto::opentelemetry::proto::logs::v1::{LogRecord, ResourceLogs};
use crate::proto::opentelemetry::proto::metrics::v1::{Metric, ResourceMetrics};
use crate::proto::opentelemetry::proto::trace::v1::ScopeSpans;
use crate::proto::opentelemetry::proto::trace::v1::span::SpanKind;
use crate::proto::opentelemetry::proto::trace::v1::{ResourceSpans, Span};
use crate::telemetry::function_context::{FunctionContextProvider, LambdaInstanceInfo};
use crate::telemetry::function_tags_provider::FunctionTagsProvider;
use crate::telemetry::lambda_function_arn::LambdaFunctionArn;
use crate::telemetry::telemetry_service::TelemetryService;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use lambda_extension::{
    InitPhase, InitReportMetrics, InitType, LambdaTelemetry, LambdaTelemetryRecord, ReportMetrics,
    ShutdownEvent,
};
use lambda_extension::{InvokeEvent, Status, TraceContext, Tracing, TracingType};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::{Arc, Once};
use time::OffsetDateTime;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::info;
use tracing::metadata::LevelFilter;
use tracing_subscriber::EnvFilter;

mod resource_attributes_customization;
mod telemetry_handling_scenarios;

#[allow(dead_code)]
static INIT: Once = Once::new();

#[allow(dead_code)]
pub fn initialize_logging() {
    INIT.call_once(|| {
        let setting = "coralogix_aws_lambda_telemetry_exporter=trace,info";

        let env_filter = EnvFilter::builder()
            .with_default_directive(LevelFilter::INFO.into())
            .parse_lossy(setting);

        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    });
}

#[derive(Default)]
struct CoralogixClientMock {
    resource_logs: Mutex<Vec<ResourceLogs>>,
    resource_spans: Mutex<Vec<ResourceSpans>>,
    resource_metrics: Mutex<Vec<ResourceMetrics>>,
}

#[async_trait]
impl OtlpSender<Vec<ResourceLogs>> for CoralogixClientMock {
    async fn send(
        &self,
        mut resource_logs: Vec<ResourceLogs>,
    ) -> Result<OtlpExportResponse, coralogix::Error> {
        info!("OtlpSender<ResourceLogs>.send");
        self.resource_logs.lock().await.append(&mut resource_logs);
        Ok(OtlpExportResponse::Success)
    }
}

#[async_trait]
impl OtlpSender<Vec<ResourceSpans>> for CoralogixClientMock {
    async fn send(
        &self,
        mut resource_spans: Vec<ResourceSpans>,
    ) -> Result<OtlpExportResponse, coralogix::Error> {
        info!("OtlpSender<ResourceSpans>.send");
        self.resource_spans.lock().await.append(&mut resource_spans);
        Ok(OtlpExportResponse::Success)
    }
}

#[async_trait]
impl OtlpSender<Vec<ResourceMetrics>> for CoralogixClientMock {
    async fn send(
        &self,
        mut resource_metrics: Vec<ResourceMetrics>,
    ) -> Result<OtlpExportResponse, coralogix::Error> {
        info!("OtlpSender<ResourceMetrics>.send");
        self.resource_metrics
            .lock()
            .await
            .append(&mut resource_metrics);
        Ok(OtlpExportResponse::Success)
    }
}

struct FunctionTagsProviderMock {
    pub latency: tokio::time::Duration,
}

#[async_trait]
impl FunctionTagsProvider for FunctionTagsProviderMock {
    async fn obtain_function_tags(
        &self,
        _arn: &LambdaFunctionArn,
    ) -> Result<HashMap<String, String>, Error> {
        tokio::time::sleep(self.latency).await;
        Ok(HashMap::new())
    }
}

fn function_context_provider() -> FunctionContextProvider {
    FunctionContextProvider::new(
        lambda_instance_info(),
        FunctionContextProviderConfig {
            tag_cache_validity: time::Duration::seconds(10),
            configured_application: None,
            configured_subsystem: None,
            configured_service_name: None,
        },
        None,
    )
}

fn function_context_provider_with_tags() -> FunctionContextProvider {
    FunctionContextProvider::new(
        lambda_instance_info(),
        FunctionContextProviderConfig {
            tag_cache_validity: time::Duration::seconds(10),
            configured_application: None,
            configured_subsystem: None,
            configured_service_name: None,
        },
        Some(Arc::new(FunctionTagsProviderMock {
            latency: tokio::time::Duration::from_millis(100),
        })),
    )
}

fn lambda_instance_info() -> LambdaInstanceInfo {
    LambdaInstanceInfo {
        aws_region: "eu-west-1".to_owned(),
        lambda_function_name: "my-lambda".to_owned(),
        lambda_function_version: "7".to_owned(),
        lambda_instance_coralogix_id: "abcd".to_owned(),
    }
}

fn handle_invoke_event(
    ts: &Arc<TelemetryService>,
    request_id: &str,
    tracing: &str,
) -> JoinHandle<()> {
    let ts = ts.clone();
    let request_id = request_id.to_owned();
    let tracing = tracing.to_owned();
    tokio::task::spawn(async move {
        ts.handle_invoke_event(invoke_event(request_id, tracing))
            .await
            .expect("handle_invoke_event should always succeed")
    })
}

fn handle_shutdown_event(ts: &Arc<TelemetryService>) -> JoinHandle<()> {
    let ts = ts.clone();
    tokio::task::spawn(async move {
        ts.handle_shutdown_event(shutdown_event())
            .await
            .expect("handle_shutdown_event should always succeed")
    })
}

fn make_telemetry_service(
    function_context_provider: FunctionContextProvider,
    coralogix_client_mock: Option<Arc<CoralogixClientMock>>,
) -> Arc<TelemetryService> {
    make_telemetry_service_with_config(
        function_context_provider,
        coralogix_client_mock,
        telemetry_service_config(
            TelemetryReportingStrategy::ReportAfterInvocation,
            TracingMode::TelemetryApi,
        ),
    )
}

fn make_telemetry_service_otel(
    function_context_provider: FunctionContextProvider,
    coralogix_client_mock: Option<Arc<CoralogixClientMock>>,
) -> Arc<TelemetryService> {
    make_telemetry_service_with_config(
        function_context_provider,
        coralogix_client_mock,
        telemetry_service_config(
            TelemetryReportingStrategy::ReportAfterInvocation,
            TracingMode::OtelInstrumentation,
        ),
    )
}

fn make_telemetry_service_with_config(
    function_context_provider: FunctionContextProvider,
    coralogix_client_mock: Option<Arc<CoralogixClientMock>>,
    config: TelemetryServiceConfig,
) -> Arc<TelemetryService> {
    let coralogix_client = coralogix_client_mock.unwrap_or_default();
    let telemetry_sender = Arc::new(CoralogixTelemetrySender {
        logs_telemetry_sender: Arc::new(OtlpPillarTelemetrySender::new(
            "logs",
            coralogix_client.clone(),
        )),
        spans_telemetry_sender: Arc::new(OtlpPillarTelemetrySender::new(
            "spans",
            coralogix_client.clone(),
        )),
        metrics_telemetry_sender: Arc::new(OtlpPillarTelemetrySender::new(
            "metrics",
            coralogix_client,
        )),
        epsagon_traces_telemetry_sender: Arc::new(NoopEpsagonTracesTelemetrySender {}),
    });

    Arc::new(TelemetryService::new(
        Arc::new(config),
        function_context_provider,
        telemetry_sender,
    ))
}

fn telemetry_service_config(
    reporting_strategy: TelemetryReportingStrategy,
    tracing_mode: TracingMode,
) -> TelemetryServiceConfig {
    TelemetryServiceConfig {
        reporting_strategy,
        reporting_delay: std::time::Duration::from_millis(200),
        max_shutdown_flush_delay: std::time::Duration::from_millis(300),
        span_sending_threshold: 2048,
        processor_config: TelemetryProcessorConfig {
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
            tracing_mode,
            trace_sampling_mode: TraceSamplingMode::All,
            message_size_limit: 30000,
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
        },
    }
}

fn timestamp() -> DateTime<Utc> {
    chrono::offset::Utc::now()
}

fn invoke_event(request_id: String, tracing: String) -> InvokeEvent {
    InvokeEvent {
        deadline_ms: 1234,
        request_id,
        invoked_function_arn: "arn:aws:lambda:eu-west-1:200000000000:function:my-lambda:my-alias"
            .to_owned(),
        tracing: Tracing {
            r#type: "X-Amzn-Trace-Id".to_owned(),
            value: tracing,
        },
    }
}

fn shutdown_event() -> ShutdownEvent {
    ShutdownEvent {
        shutdown_reason: "TIMEOUT".to_owned(),
        deadline_ms: ((OffsetDateTime::now_utc() + time::Duration::milliseconds(2000))
            .unix_timestamp_nanos()
            / 1_000_000) as u64,
    }
}

fn init_start() -> LambdaTelemetry {
    LambdaTelemetry {
        time: timestamp(),
        record: LambdaTelemetryRecord::PlatformInitStart {
            initialization_type: InitType::OnDemand,
            phase: InitPhase::Init,
            runtime_version: Some("java:11.v18".to_owned()),
            runtime_version_arn: Some("arn:aws:lambda:eu-west-1::runtime:62f37f50595aa92b7961bc3096ff0fbccd683915c384a9c3bf0d9b2cc3c5e90f".to_owned()) 
        },
    }
}

fn init_runtime_done() -> LambdaTelemetry {
    LambdaTelemetry {
        time: timestamp(),
        record: LambdaTelemetryRecord::PlatformInitRuntimeDone {
            initialization_type: InitType::OnDemand,
            phase: Some(InitPhase::Init),
            status: Status::Success,
            error_type: None,
            spans: Vec::new(),
        },
    }
}

fn init_runtime_done_failed() -> LambdaTelemetry {
    LambdaTelemetry {
        time: timestamp(),
        record: LambdaTelemetryRecord::PlatformInitRuntimeDone {
            initialization_type: InitType::OnDemand,
            phase: Some(InitPhase::Init),
            status: Status::Error,
            error_type: Some("Runtime.ExitError".to_owned()),
            spans: Vec::new(),
        },
    }
}

fn init_report() -> LambdaTelemetry {
    LambdaTelemetry {
        time: timestamp(),
        record: LambdaTelemetryRecord::PlatformInitReport {
            initialization_type: InitType::OnDemand,
            phase: InitPhase::Init,
            metrics: InitReportMetrics { duration_ms: 0.0 },
            spans: Vec::new(),
        },
    }
}

fn platform_start(request_id: &str, trace_context: &TraceContext) -> LambdaTelemetry {
    LambdaTelemetry {
        time: timestamp(),
        record: LambdaTelemetryRecord::PlatformStart {
            request_id: request_id.to_owned(),
            version: Some(VERSION.to_owned()),
            tracing: Some(trace_context.clone()),
        },
    }
}

fn platform_runtime_done(request_id: &str, trace_context: &TraceContext) -> LambdaTelemetry {
    LambdaTelemetry {
        time: timestamp(),
        record: LambdaTelemetryRecord::PlatformRuntimeDone {
            request_id: request_id.to_owned(),
            tracing: Some(trace_context.clone()),
            status: Status::Success,
            error_type: None,
            metrics: None,     // TODO add realistic metrics
            spans: Vec::new(), // TODO add realistic spans
        },
    }
}

fn platform_report(request_id: &str, trace_context: &TraceContext) -> LambdaTelemetry {
    LambdaTelemetry {
        time: timestamp(),
        record: LambdaTelemetryRecord::PlatformReport {
            request_id: request_id.to_owned(),
            tracing: Some(trace_context.clone()),
            status: Status::Success,
            error_type: None,
            metrics: ReportMetrics {
                duration_ms: 0.0,
                billed_duration_ms: 0,
                memory_size_mb: 0,
                max_memory_used_mb: 0,
                init_duration_ms: Some(0.0),
                restore_duration_ms: None,
            },
            spans: Vec::new(),
        },
    }
}

fn function_log(text: &str) -> LambdaTelemetry {
    LambdaTelemetry {
        time: timestamp(),
        record: LambdaTelemetryRecord::Function(text.to_owned()),
    }
}

fn main_span_export() -> ExportTraceServiceRequest {
    ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            scope_spans: vec![ScopeSpans {
                spans: vec![Span {
                    name: "main function span".to_owned(),
                    trace_id: vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
                    span_id: vec![0, 0, 0, 0, 0, 0, 0, 1],
                    kind: SpanKind::Server as i32,
                    ..Span::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

fn single_span_export(span_name: &str) -> ExportTraceServiceRequest {
    ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            scope_spans: vec![ScopeSpans {
                spans: vec![Span {
                    name: span_name.to_owned(),
                    ..Span::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

async fn expect_readiness(ready_to_freez: JoinHandle<()>) {
    tokio::time::timeout(tokio::time::Duration::from_millis(500), ready_to_freez)
        .await
        .expect("Should signalize readiness to freez/shutdown")
        .unwrap();
}

async fn give_events_time_to_process() {
    sleep_ms(50).await;
}

async fn sleep_ms(ms: u64) {
    tokio::time::sleep(tokio::time::Duration::from_millis(ms)).await;
}

fn assert_belong_to_one_span(logs: &[LogRecord]) -> (Vec<u8>, Vec<u8>) {
    if logs.is_empty() {
        panic!("Empty list of logs")
    }
    let span_id = &logs[0].span_id;
    let span_ref = (logs[0].trace_id.clone(), logs[0].span_id.clone());

    assert_belong_to_one_trace(logs);

    for (i, log) in logs.iter().enumerate() {
        if &log.span_id != span_id {
            panic!(
                "Expected log number {} to have the same span_id as other logs {}, but it has span_id {}",
                i,
                hex::encode(span_id),
                hex::encode(&log.span_id)
            )
        }
    }
    span_ref
}

fn assert_belong_to_one_trace(logs: &[LogRecord]) -> Vec<u8> {
    if logs.is_empty() {
        panic!("Empty list of logs")
    }
    let trace_id = &logs[0].trace_id;
    for (i, log) in logs.iter().enumerate() {
        if &log.trace_id != trace_id {
            panic!(
                "Expected log number {} to have the same trace_id as other logs {}, but it has trace_id {}",
                i,
                hex::encode(trace_id),
                hex::encode(&log.trace_id)
            )
        }
    }
    trace_id.clone()
}

fn assert_log_contains(log: &LogRecord, str: &str) {
    assert!(format!("{:?}", log.body.as_ref().unwrap()).contains(str));
}

async fn extract_log_records(coralogix: &CoralogixClientMock) -> Vec<LogRecord> {
    let resource_logs = coralogix.resource_logs.lock().await.clone();
    resource_logs
        .into_iter()
        .flat_map(|x| x.scope_logs.into_iter())
        .flat_map(|x| x.log_records)
        .collect()
}

async fn extract_spans(coralogix: &CoralogixClientMock) -> Vec<Span> {
    let resource_spans = coralogix.resource_spans.lock().await.clone();
    resource_spans
        .into_iter()
        .flat_map(|x| x.scope_spans.into_iter())
        .flat_map(|x| x.spans)
        .collect()
}

async fn extract_metrics(coralogix: &CoralogixClientMock) -> Vec<Metric> {
    let resource_metrics = coralogix.resource_metrics.lock().await.clone();
    resource_metrics
        .into_iter()
        .flat_map(|x| x.scope_metrics.into_iter())
        .flat_map(|x| x.metrics)
        .collect()
}

async fn expect_no_telemetry(coralogix: &CoralogixClientMock) {
    assert_eq!(extract_log_records(coralogix).await.len(), 0);
    assert_eq!(extract_spans(coralogix).await.len(), 0);
    assert_eq!(extract_metrics(coralogix).await.len(), 0);
}

const REQUEST1_ID: &str = "request1";
// TODO verify how TRACING1 and TRACE_CONTEXT1 should relate
const TRACING1: &str = "Root=1-6352a70e-1e2c502e358361800241fd41;Parent=35465b3a9e2f7c61;Sampled=1";
lazy_static! {
    static ref TRACE_CONTEXT1: TraceContext = TraceContext {
        span_id: Some("abcd1".to_owned()),
        r#type: TracingType::AmznTraceId,
        value: "Root=1-6352a70e-1e2c502e358361800241fd41;Parent=35465b3a9e2f7c61;Sampled=1"
            .to_owned(),
    };
}

const REQUEST2_ID: &str = "request2";
// TODO verify how TRACING2 and TRACE_CONTEXT2 should relate
const TRACING2: &str = "Root=1-6352a70e-1e2c502e358361800241fd42;Parent=35465b3a9e2f7c62;Sampled=1";
lazy_static! {
    static ref TRACE_CONTEXT2: TraceContext = TraceContext {
        span_id: Some("abcd2".to_owned()),
        r#type: TracingType::AmznTraceId,
        value: "Root=1-6352a70e-1e2c502e358361800241fd42;Parent=35465b3a9e2f7c62;Sampled=1"
            .to_owned(),
    };
}

const REQUEST3_ID: &str = "request3";
// TODO verify how TRACING2 and TRACE_CONTEXT2 should relate
const TRACING3: &str = "Root=1-6352a70e-1e2c502e358361800241fd43;Parent=35465b3a9e2f7c63;Sampled=1";
lazy_static! {
    static ref TRACE_CONTEXT3: TraceContext = TraceContext {
        span_id: Some("abcd2".to_owned()),
        r#type: TracingType::AmznTraceId,
        value: "Root=1-6352a70e-1e2c502e358361800241fd43;Parent=35465b3a9e2f7c63;Sampled=1"
            .to_owned(),
    };
}

const VERSION: &str = "7"; // TODO: not sure what AWS would provide in PlatformStart event "my-alias" or "7".
