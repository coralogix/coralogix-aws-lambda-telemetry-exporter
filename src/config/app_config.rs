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

use super::*;
use crate::api_key::ApiKey;
use itertools::Itertools;
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashMap;
use std::env;
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn};

const LOGS: &str = "LOGS";
const TRACES: &str = "TRACES";
const METRICS: &str = "METRICS";

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub target: TargetConfigs,
    pub aws_telemetry_interval_ms: usize,
    pub otlp_server_enabled: bool,
    pub tags_enabled: bool,
    pub telemetry_service_config: TelemetryServiceConfig,
    pub function_context_provider_config: FunctionContextProviderConfig,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct TargetConfigs {
    pub logs: Option<OtlpTargetConfig>,
    pub traces: Option<OtlpTargetConfig>,
    pub metrics: Option<OtlpTargetConfig>,
    pub main: Option<TargetConfig>,
    pub alpn_enabled: bool,
    pub combined_telemetry_enabled: bool,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum TargetConfig {
    Otlp { target: OtlpTargetConfig },
    Firehose { delivery_stream_name: String },
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum OtlpTargetConfig {
    Coralogix {
        domain: String,
        key_source_config: KeySourceConfig,
    },
    Otel {
        url: String,
    },
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum KeySourceConfig {
    EnvVar { key: ApiKey },
    SecretsManager { secret_id: String },
}

#[derive(Debug, Clone)]
pub struct TelemetryServiceConfig {
    pub reporting_strategy: TelemetryReportingStrategy,
    pub reporting_delay: Duration,
    pub max_shutdown_flush_delay: Duration,
    pub span_sending_threshold: usize,
    pub processor_config: TelemetryProcessorConfig,
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum TelemetryReportingStrategy {
    LowOverhead,
    ReportAfterInvocation,
    ReportDuringAndAfterInvocation,
}

impl TelemetryReportingStrategy {
    pub fn send_after_invocation(&self) -> bool {
        match self {
            TelemetryReportingStrategy::LowOverhead => false,
            TelemetryReportingStrategy::ReportAfterInvocation => true,
            TelemetryReportingStrategy::ReportDuringAndAfterInvocation => true,
        }
    }

    pub fn send_after_delay(&self) -> bool {
        match self {
            TelemetryReportingStrategy::LowOverhead => true,
            TelemetryReportingStrategy::ReportAfterInvocation => false,
            TelemetryReportingStrategy::ReportDuringAndAfterInvocation => true,
        }
    }
}

impl FromStr for TelemetryReportingStrategy {
    type Err = AppConfigError;

    fn from_str(s: &str) -> Result<Self, AppConfigError> {
        // numeric values are supported as a way to reduce the size of the config
        match s.trim().to_lowercase().as_str() {
            "low_overhead" | "1" => Ok(TelemetryReportingStrategy::LowOverhead),
            "report_after_invocation" | "2" => {
                Ok(TelemetryReportingStrategy::ReportAfterInvocation)
            }
            "report_during_and_after_invocation" | "3" => {
                Ok(TelemetryReportingStrategy::ReportDuringAndAfterInvocation)
            }
            _ => Err(AppConfigError::InvalidReportingStrategy { name: s.to_owned() }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TelemetryProcessorConfig {
    pub logs_metadata_mode: LogsMetadataMode,
    pub log_mode: LogMode,
    pub platform_log_set: PlatformEventLogSet,
    pub platform_logs: PlatformLogsConfig,
    pub platform_metrics_mode: PlatformMetricsMode,
    pub otel_metrics_mode: OtelMetricsMode,
    pub tracing_mode: TracingMode,
    pub trace_sampling_mode: TraceSamplingMode,
    pub message_size_limit: usize,
    pub excluded_span_attributes: AttributeExclusionMode,
    pub resource_attributes: AttributeConfigs,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum LogsMetadataMode {
    Disabled,
    Enabled(LogMetadataConfig),
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct LogMetadataConfig {
    pub include_trace_ref: bool,
    pub include_execution: bool,
    pub include_invocation_id: bool,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct AttributeConfigs {
    pub logs: AttributeConfig,
    pub traces: AttributeConfig,
    pub metrics: AttributeConfig,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct AttributeConfig {
    pub built_in: BuiltInAttributeSet,
    pub extra: HashMap<String, String>,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct BuiltInAttributeSet {
    // cx.application.name and cx.subsystem.name cannot be disabled
    pub service_name: bool,
    pub cloud_provider: bool,
    pub cloud_account_id: bool,
    pub cloud_region: bool,
    pub faas_name: bool,
    pub faas_id: bool,
    pub faas_instance_cx_id: bool,
}

impl BuiltInAttributeSet {
    // I don't want to use a derived Default here, because the "default configuration" is not an empty BuiltInAttributeSet
    pub fn empty() -> BuiltInAttributeSet {
        BuiltInAttributeSet {
            service_name: false,
            cloud_provider: false,
            cloud_account_id: false,
            cloud_region: false,
            faas_name: false,
            faas_id: false,
            faas_instance_cx_id: false,
        }
    }

    pub fn all() -> BuiltInAttributeSet {
        BuiltInAttributeSet {
            service_name: true,
            cloud_provider: true,
            cloud_account_id: true,
            cloud_region: true,
            faas_name: true,
            faas_id: true,
            faas_instance_cx_id: true,
        }
    }
}

impl FromStr for BuiltInAttributeSet {
    type Err = AppConfigError;

    fn from_str(s: &str) -> Result<Self, AppConfigError> {
        let tokens = s.split(',');
        let mut result = BuiltInAttributeSet::empty();
        for token in tokens {
            match token.trim().to_lowercase().replace(".", "_").as_str() {
                "" => (),
                "service_name" | "1" => result.service_name = true,
                "cloud_provider" | "2" => result.cloud_provider = true,
                "cloud_account_id" | "3" => result.cloud_account_id = true,
                "cloud_region" | "4" => result.cloud_region = true,
                "faas_name" | "5" => result.faas_name = true,
                "faas_id" | "6" => result.faas_id = true,
                "faas_instance_cx_id" | "7" => result.faas_instance_cx_id = true,
                other => Err(AppConfigError::InvalidBuiltInAttribute {
                    name: other.to_owned(),
                })?,
            }
        }
        Ok(result)
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum LogMode {
    Disabled = 1,
    Structured = 2,
}

impl FromStr for LogMode {
    type Err = AppConfigError;

    fn from_str(s: &str) -> Result<Self, AppConfigError> {
        match s.trim().to_lowercase().as_str() {
            "disabled" | "1" => Ok(LogMode::Disabled),
            "structured" | "2" => Ok(LogMode::Structured),
            other => Err(AppConfigError::InvalidLogMode {
                name: other.to_owned(),
            }),
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct PlatformEventLogSet {
    pub start: bool,
    pub runtime_done: bool,
    pub report: bool,
}

impl PlatformEventLogSet {
    pub fn empty() -> PlatformEventLogSet {
        PlatformEventLogSet {
            start: false,
            runtime_done: false,
            report: false,
        }
    }

    pub fn all() -> PlatformEventLogSet {
        PlatformEventLogSet {
            start: true,
            runtime_done: true,
            report: true,
        }
    }
}

impl FromStr for PlatformEventLogSet {
    type Err = AppConfigError;

    fn from_str(s: &str) -> Result<Self, AppConfigError> {
        match s.trim().to_lowercase().as_str() {
            "disabled" | "0" | "" => Ok(PlatformEventLogSet::empty()),
            other => {
                let tokens = other.split(',');
                let mut result = PlatformEventLogSet::empty();
                for token in tokens {
                    match token.trim() {
                        "start" | "1" => result.start = true,
                        "runtime_done" | "2" => result.runtime_done = true,
                        "report" | "3" => result.report = true,
                        other => Err(AppConfigError::InvalidPlatformEventLog {
                            name: other.to_owned(),
                        })?,
                    }
                }
                Ok(result)
            }
        }
    }
}
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PlatformLogsConfig {
    pub include_request_id: bool,
    pub hide_default_values: bool,
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum PlatformMetricsMode {
    Disabled = 1,
    PlatformReport = 2,
    PlatformV2 = 3,
}

impl FromStr for PlatformMetricsMode {
    type Err = AppConfigError;

    fn from_str(s: &str) -> Result<Self, AppConfigError> {
        match s.trim().to_lowercase().as_str() {
            "disabled" | "1" => Ok(PlatformMetricsMode::Disabled),
            "platform_report" | "2" => Ok(PlatformMetricsMode::PlatformReport),
            "platform_v2" | "3" => Ok(PlatformMetricsMode::PlatformV2),
            other => Err(AppConfigError::InvalidMetricsMode {
                name: other.to_owned(),
            }),
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum OtelMetricsMode {
    Disabled = 1,
    Direct = 2,
    Processed = 3,
}

impl FromStr for OtelMetricsMode {
    type Err = AppConfigError;

    fn from_str(s: &str) -> Result<Self, AppConfigError> {
        match s.trim().to_lowercase().as_str() {
            "disabled" | "1" => Ok(OtelMetricsMode::Disabled),
            "direct" | "2" => Ok(OtelMetricsMode::Direct),
            "processed" | "3" => Ok(OtelMetricsMode::Processed),
            other => Err(AppConfigError::InvalidOtelMetricsMode {
                name: other.to_owned(),
            }),
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum TracingMode {
    Disabled = 1,
    TelemetryApi = 2,
    OtelInstrumentation = 3,
}

impl FromStr for TracingMode {
    type Err = AppConfigError;

    fn from_str(s: &str) -> Result<Self, AppConfigError> {
        match s.trim().to_lowercase().as_str() {
            "disabled" | "1" => Ok(TracingMode::Disabled),
            "telemetry_api" | "2" => Ok(TracingMode::TelemetryApi),
            // "opentelemetry_instrumentation" is supported for backward compatibility with pre 0.3.0 internal preview releases.
            // "otel" has been introduced to reduce the config size
            "otel" | "opentelemetry_instrumentation" | "3" => Ok(TracingMode::OtelInstrumentation),
            other => Err(AppConfigError::InvalidTracingMode {
                name: other.to_owned(),
            }),
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum TraceSamplingMode {
    All = 1,
    FollowXray = 2,
}

impl FromStr for TraceSamplingMode {
    type Err = AppConfigError;

    fn from_str(s: &str) -> Result<Self, AppConfigError> {
        match s.trim().to_lowercase().as_str() {
            "all" | "1" => Ok(TraceSamplingMode::All),
            "follow_xray" | "2" => Ok(TraceSamplingMode::FollowXray),
            other => Err(AppConfigError::InvalidTraceSamplingMode {
                name: other.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub enum AttributeExclusionMode {
    Disabled,
    Predefined,
    Regex(Regex),
}

impl AttributeExclusionMode {
    pub fn regex(&self) -> Option<&Regex> {
        match self {
            AttributeExclusionMode::Disabled => None,
            AttributeExclusionMode::Predefined => Some(&PREDEFINED_ATTRIBUTE_EXCLUSION_REGEX),
            AttributeExclusionMode::Regex(regex) => Some(regex),
        }
    }
}

lazy_static! {
    static ref PREDEFINED_ATTRIBUTE_EXCLUSION_REGEX: Regex =
        Regex::new(r"http\.request\.header\.x-api-key|http\.request\.header\.authorization")
            .unwrap();
}

impl FromStr for AttributeExclusionMode {
    type Err = AppConfigError;

    fn from_str(s: &str) -> Result<Self, AppConfigError> {
        match s.trim().to_lowercase().as_str() {
            "disabled" | "1" => Ok(AttributeExclusionMode::Disabled),
            "predefined" | "2" => Ok(AttributeExclusionMode::Predefined),
            _ => match Regex::new(s.trim()) {
                Ok(r) => Ok(AttributeExclusionMode::Regex(r)),
                Err(_) => Err(AppConfigError::InvalidAttributeExclusionMode {
                    value: s.to_owned(),
                }),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct FunctionContextProviderConfig {
    pub configured_application: Option<String>,
    pub configured_subsystem: Option<String>,
    pub configured_service_name: Option<String>,
    pub tag_cache_validity: time::Duration,
}

impl AppConfig {
    // Because this is an AWS Lambda extension, the primary way of configuring it is via env vars.
    // Because of that the SCREAMING_SNAKE_CASE convention is used.
    // The namespace is shared with the function and other extensions, hence the need for the CX/CORALOGIX prefix.
    // Te same could be achieved by `Config` and `Environment::with_prefix` but then the error messages end up using snake_case without the prefix which is poor user experience.
    // (Keep in mind, that unlike in backend services, here, the customers see the configuration error message)
    pub fn load() -> Result<Self, AppConfigError> {
        let reporting_strategy =
            get_string("REPORTING_STRATEGY")?.parse::<TelemetryReportingStrategy>()?;

        let log_mode = get_parsable("LOG_MODE", LogMode::Structured)?;
        let logs_enabled = !matches!(log_mode, LogMode::Disabled);

        let tracing_mode = extract_tracing_mode()?;
        let traces_enabled = !matches!(tracing_mode, TracingMode::Disabled);

        let platform_metrics_mode = extract_platform_metrics_mode()?;
        let otel_metrics_mode = get_parsable("OTEL_METRICS_MODE", OtelMetricsMode::Processed)?;
        let metrics_enabled = !matches!(platform_metrics_mode, PlatformMetricsMode::Disabled)
            || !matches!(otel_metrics_mode, OtelMetricsMode::Disabled);

        let config = AppConfig {
            target: extract_target_configs(logs_enabled, traces_enabled, metrics_enabled)?,
            aws_telemetry_interval_ms: extract_aws_telemetry_interval(reporting_strategy)?,
            otlp_server_enabled: get_bool_option("OTLP_SERVER_ENABLED")?.unwrap_or(true),
            tags_enabled: get_bool_option("TAGS_ENABLED")?.unwrap_or(false),
            telemetry_service_config: TelemetryServiceConfig {
                reporting_strategy,
                reporting_delay: get_duration_ms("REPORTING_DELAY_MS", 15000)?,
                max_shutdown_flush_delay: get_duration_ms("MAX_SHUTDOWN_FLUSH_DELAY_MS", 600)?,
                span_sending_threshold: get_usize_option("SPAN_SENDING_THRESHOLD")?.unwrap_or(2048),
                processor_config: TelemetryProcessorConfig {
                    logs_metadata_mode: extract_logs_metadata_mode()?,
                    log_mode,
                    platform_log_set: get_parsable("PLATFORM_LOGS", PlatformEventLogSet::all())?,
                    platform_logs: PlatformLogsConfig {
                        include_request_id: get_bool_option("PLATFORM_LOGS_INCLUDE_REQUEST_ID")?
                            .unwrap_or(true),
                        hide_default_values: get_bool_option("PLATFORM_LOGS_HIDE_DEFAULT_VALUES")?
                            .unwrap_or(false),
                    },
                    platform_metrics_mode,
                    otel_metrics_mode,
                    tracing_mode,
                    trace_sampling_mode: get_parsable(
                        "TRACE_SAMPLING_MODE",
                        TraceSamplingMode::All,
                    )?,
                    message_size_limit: get_usize_option("MESSAGE_SIZE_LIMIT")?.unwrap_or(30_000),
                    excluded_span_attributes: get_parsable(
                        "EXCLUDED_SPAN_ATTRIBUTES",
                        AttributeExclusionMode::Disabled,
                    )?,
                    resource_attributes: extract_attribute_configs(
                        metrics_enabled,
                        traces_enabled,
                    )?,
                },
            },
            function_context_provider_config: FunctionContextProviderConfig {
                configured_application: get_string_option("APPLICATION"),
                configured_subsystem: get_string_option("SUBSYSTEM"),
                configured_service_name: get_string_option("SERVICE_NAME"),
                tag_cache_validity: get_i64_option("TAG_CACHE_VALIDITY_MS")?.map_or_else(
                    || time::Duration::milliseconds(5 * 60_000),
                    time::Duration::milliseconds,
                ),
            },
        };

        check_for_known_configuration_issues(&config);
        Ok(config)
    }
}

fn extract_target_configs(
    logs_enabled: bool,
    traces_enabled: bool,
    metrics_enabled: bool,
) -> Result<TargetConfigs, AppConfigError> {
    let alpn_enabled = get_bool_option("OTEL_ALPN_ENABLED")?.unwrap_or(false);
    let combined_telemetry_enabled =
        get_bool_option("COMBINED_TELEMETRY_ENABLED")?.unwrap_or(false);
    let main_target = match (
        get_string_option("FIREHOSE"),
        get_string_option("DOMAIN"),
        get_string_option("OTEL_URL"),
    ) {
        (None, Some(domain), None) => {
            let cx = OtlpTargetConfig::Coralogix {
                domain,
                key_source_config: extract_key_source_config()?,
            };
            Ok(Some(TargetConfig::Otlp { target: cx }))
        }
        (None, None, Some(otel_url)) => {
            let otlp = OtlpTargetConfig::Otel { url: otel_url };
            Ok(Some(TargetConfig::Otlp { target: otlp }))
        }
        (Some(delivery_stream_name), None, None) => Ok(Some(TargetConfig::Firehose {
            delivery_stream_name,
        })),
        (Some(_), _, Some(_)) => Err(AppConfigError::FirehoseConflict()),
        (Some(_), Some(_), _) => Err(AppConfigError::FirehoseConflict()),
        (_, Some(_), Some(_)) => Err(AppConfigError::UrlConflict()),
        (None, None, None) => Ok(None),
    }?;

    let logs_target = extract_target_config(LOGS);
    let traces_target = extract_target_config(TRACES);
    let metrics_target = extract_target_config(METRICS);

    if logs_enabled && main_target.is_none() && logs_target.is_none() {
        return Err(AppConfigError::MissingTarget {
            pillar: LOGS.to_owned(),
        });
    }

    if traces_enabled && main_target.is_none() && traces_target.is_none() {
        return Err(AppConfigError::MissingTarget {
            pillar: TRACES.to_owned(),
        });
    }

    if metrics_enabled && main_target.is_none() && metrics_target.is_none() {
        return Err(AppConfigError::MissingTarget {
            pillar: METRICS.to_owned(),
        });
    }

    Ok(TargetConfigs {
        logs: logs_target,
        traces: traces_target,
        metrics: metrics_target,
        main: main_target,
        alpn_enabled,
        combined_telemetry_enabled,
    })
}

fn extract_target_config(pillar: &'static str) -> Option<OtlpTargetConfig> {
    get_string_option(&format!("OTEL_{pillar}_URL")).map(|url| OtlpTargetConfig::Otel { url })
}

fn extract_key_source_config() -> Result<KeySourceConfig, AppConfigError> {
    match (
        get_string_option("API_KEY").or_else(|| get_string_option("PRIVATE_KEY")),
        get_string_option("SECRET"),
    ) {
        (Some(key), None) => Ok(KeySourceConfig::EnvVar {
            key: ApiKey::from(key),
        }),
        (None, Some(secret_id)) => Ok(KeySourceConfig::SecretsManager { secret_id }),
        (Some(_), Some(_)) => Err(AppConfigError::KeySourceConflict()),
        (None, None) => Err(AppConfigError::MissingKeySource()),
    }
}

fn extract_aws_telemetry_interval(
    reporting_strategy: TelemetryReportingStrategy,
) -> Result<usize, AppConfigError> {
    match (
        get_usize_option("AWS_TELEMETRY_INTERVAL_MS")?,
        reporting_strategy,
    ) {
        (Some(interval_ms), _) if (!(25..=30000).contains(&interval_ms)) => {
            Err(AppConfigError::InvalidTelemetryInterval { value: interval_ms })
        }
        (Some(interval_ms), _) => Ok(interval_ms),
        (None, TelemetryReportingStrategy::LowOverhead) => Ok(500),
        (None, TelemetryReportingStrategy::ReportAfterInvocation) => Ok(25),
        (None, TelemetryReportingStrategy::ReportDuringAndAfterInvocation) => Ok(500),
    }
}

fn extract_logs_metadata_mode() -> Result<LogsMetadataMode, AppConfigError> {
    let enabled = get_bool_option("LOGS_METADATA_ENABLED")
        .transpose()
        .or_else(|| get_bool_option("LOG_METADATA_ENABLED").transpose())
        .unwrap_or(Ok(true))?;
    let include_trace_ref = get_bool_option("LOGS_METADATA_INCLUDE_TRACE_REF")?;
    let include_execution = get_bool_option("LOGS_METADATA_INCLUDE_EXECUTION")?;
    let include_invocation_id = get_bool_option("LOGS_METADATA_INCLUDE_INVOCATION_ID")?;

    if enabled {
        Ok(LogsMetadataMode::Enabled(LogMetadataConfig {
            include_trace_ref: include_trace_ref.unwrap_or(true),
            include_execution: include_execution.unwrap_or(true),
            include_invocation_id: include_invocation_id.unwrap_or(false),
        }))
    } else {
        if include_trace_ref.is_some() {
            info!(
                "CX_LOGS_METADATA_INCLUDE_TRACE_REF has no effect when CX_LOGS_METADATA_ENABLED=false"
            );
        }
        if include_execution.is_some() {
            info!(
                "CX_LOGS_METADATA_INCLUDE_EXECUTION has no effect when CX_LOGS_METADATA_ENABLED=false"
            );
        }
        if include_invocation_id.is_some() {
            info!(
                "CX_LOGS_METADATA_INCLUDE_INVOCATION_ID has no effect when CX_LOGS_METADATA_ENABLED=false"
            );
        }
        Ok(LogsMetadataMode::Disabled)
    }
}

fn extract_platform_metrics_mode() -> Result<PlatformMetricsMode, AppConfigError> {
    match (
        get_string_option("METRICS_MODE")
            .map(|s| s.parse::<PlatformMetricsMode>())
            .transpose()?,
        get_bool_option("METRICS_ENABLED")?,
        get_bool_option("LOG_ONLY")?.unwrap_or(false),
    ) {
        (None, None, false) => Ok(PlatformMetricsMode::PlatformReport),
        (None, Some(true), false) => {
            warn!(
                "CX_METRICS_ENABLED is deprecated, please use CX_METRICS_MODE=platform_report instead, or remove the variable (the current setting is identical to the default)"
            );
            Ok(PlatformMetricsMode::PlatformReport)
        }
        (None, Some(false), false) => {
            warn!("CX_METRICS_ENABLED is deprecated, please use CX_METRICS_MODE=disabled instead");
            Ok(PlatformMetricsMode::Disabled)
        }
        (Some(mode), None, false) => Ok(mode),
        (Some(_), Some(_), _) => Err(AppConfigError::MetricsModeConflict()),
        (None, None, true) => Ok(PlatformMetricsMode::Disabled),
        (Some(PlatformMetricsMode::Disabled), None, true) => {
            info!("CX_METRICS_MODE=disabled is redundant when CX_LOG_ONLY=true");
            Ok(PlatformMetricsMode::Disabled)
        }
        (None, Some(false), true) => {
            info!("CX_METRICS_ENABLED=false is redundant when CX_LOG_ONLY=true");
            Ok(PlatformMetricsMode::Disabled)
        }
        (_, _, true) => Err(AppConfigError::LogOnlyMetricsConflict()),
    }
}

fn extract_tracing_mode() -> Result<TracingMode, AppConfigError> {
    // We detect if OTEL auto-instrumentation is in use. If it is, then the telemetry-exporter will default to otel tracing mode.
    let aws_wrapper_var = env::var("AWS_LAMBDA_EXEC_WRAPPER").ok();
    let has_otel_wrapper_file = std::fs::metadata("/opt/otel-handler").is_ok();

    match (
        get_string_option("TRACING_MODE")
            .map(|s| s.parse::<TracingMode>())
            .transpose()?,
        get_bool_option("LOG_ONLY")?.unwrap_or(false),
        aws_wrapper_var.as_deref(),
        has_otel_wrapper_file,
    ) {
        (None, true, _, _) => Ok(TracingMode::Disabled),
        (None, false, Some("/opt/otel-handler"), true) => {
            debug!("Using OTEL tracing mode because otel-handler is in use.");
            Ok(TracingMode::OtelInstrumentation)
        }
        (None, false, _, true) => {
            warn!(
                "OTEL instrumentation is present in the filesystem, but is not enabled. Please consider configuring AWS_LAMBDA_EXEC_WRAPPER=/opt/otel-handler"
            );
            Ok(TracingMode::TelemetryApi)
        }
        (None, false, _, false) => {
            debug!("Using telemetry-api tracing mode because otel-handler is not present.");
            Ok(TracingMode::TelemetryApi)
        }
        (Some(TracingMode::Disabled), true, _, _) => {
            info!("CX_TRACING_MODE=disabled is redundant when CX_LOG_ONLY=true");
            Ok(TracingMode::Disabled)
        }
        (Some(_), true, _, _) => Err(AppConfigError::LogOnlyTracingConflict()),
        (Some(mode), false, _, _) => Ok(mode),
    }
}

fn extract_attribute_configs(
    metrics_enabled: bool,
    traces_enabled: bool,
) -> Result<AttributeConfigs, AppConfigError> {
    let mut traces = extract_attribute_config(TRACES)?;
    if !traces.built_in.service_name {
        traces.built_in.service_name = true;
        if traces_enabled {
            info!(
                "service_name attribute is disabled for traces in current configuration, but this will be ignored, because service_name is mandator for traces in Coralogix."
            );
        }
    }

    let mut metrics = extract_attribute_config(METRICS)?;
    if !metrics.built_in.faas_instance_cx_id {
        metrics.built_in.faas_instance_cx_id = true;
        if metrics_enabled {
            info!(
                "faas.instance.cx_id attribute is disabled for metrics in current configuration, but this will be ignored, because without faas.instance.cx_id metrics from multiple instances cannot be correctly aggregated (https://opentelemetry.io/docs/specs/otel/metrics/data-model/#single-writer)."
            );
        }
    }

    Ok(AttributeConfigs {
        logs: extract_attribute_config(LOGS)?,
        traces,
        metrics,
    })
}

fn extract_attribute_config(pillar: &'static str) -> Result<AttributeConfig, AppConfigError> {
    Ok(AttributeConfig {
        built_in: extract_built_in_attributes(pillar)?,
        extra: extract_extra_attributes(pillar),
    })
}

fn extract_built_in_attributes(
    pillar: &'static str,
) -> Result<BuiltInAttributeSet, AppConfigError> {
    let property_name = format!("{}_RESOURCE_BUILT_IN_ATTRIBUTES", pillar);
    let s = get_string_option(&property_name)
        .or_else(|| get_string_option("RESOURCE_BUILT_IN_ATTRIBUTES"));
    s.map_or(Ok(BuiltInAttributeSet::all()), |s| {
        s.parse::<BuiltInAttributeSet>()
    })
}

fn extract_extra_attributes(pillar: &'static str) -> HashMap<String, String> {
    let property_name = format!("{}_RESOURCE_EXTRA_ATTRIBUTES", pillar);
    get_string_option(&property_name).or_else(|| get_string_option("RESOURCE_EXTRA_ATTRIBUTES"))
        .map(|s| s.split(',').flat_map(|pair| {
            let split_pair = pair.trim().split('=').collect_vec();
            match split_pair[..] {
                [""] => None,
                [key, value] => Some((key.to_owned(), value.to_owned())),
                _ => {
                    warn!("Invalid resource extra attribute pair: '{}'. Expected format: 'key=value'", pair);
                    None
                }
            }
        }).collect())
        .unwrap_or_default()
}

fn check_for_known_configuration_issues(config: &AppConfig) {
    let max_shutdown_flush_ms = config
        .telemetry_service_config
        .max_shutdown_flush_delay
        .as_millis();

    if max_shutdown_flush_ms <= config.aws_telemetry_interval_ms as u128 {
        warn!(
            "CX_MAX_SHUTDOWN_FLUSH_DELAY ({}) is less than or equal to CX_AWS_TELEMETRY_INTERVAL_MS ({}). This can lead to loosing telemetry during shutdown.",
            max_shutdown_flush_ms, config.aws_telemetry_interval_ms
        )
    }

    if max_shutdown_flush_ms >= 2000 {
        warn!(
            "CX_MAX_SHUTDOWN_FLUSH_DELAY ({}) is greater than 2000ms. This can lead to loosing telemetry during shutdown.",
            max_shutdown_flush_ms
        )
    }

    if config.telemetry_service_config.reporting_strategy
        == TelemetryReportingStrategy::ReportAfterInvocation
        && get_env_var_with_prefix("REPORTING_DELAY_MS").is_ok()
    {
        warn!(
            "CX_REPORTING_DELAY_MS is configured, but it has no effect while using REPORT_AFTER_INVOCATION reporting strategy."
        )
    }
}

#[derive(Error)]
pub enum AppConfigError {
    #[error("Environment variable CX_{key} is missing")]
    VariableMissing { key: String },
    #[error(
        "The value of environment variable CX_{key} is invalid. Expected it to be an integer, but found '{value}'"
    )]
    InvalidInteger { key: String, value: String },
    #[error(
        "The value of environment variable CX_{key} is invalid. Expected it to be a boolean (true/false/t/f), but found '{value}'"
    )]
    InvalidBoolean { key: String, value: String },
    #[error("When CX_FIREHOSE is specified, CX_DOMAIN, CX_OTEL_URL should not be specified.")]
    FirehoseConflict(),
    #[error("When CX_OTEL_URL is specified neither CX_DOMAIN nor CX_FIREHOSE should be specified.")]
    UrlConflict(),
    #[error(
        "No target specified for {pillar}. Either CX_DOMAIN, CX_OTEL_URL, CX_FIREHOSE or CX_OTEL_{pillar}_URL should be specified."
    )]
    MissingTarget { pillar: String },
    #[error("When CX_SECRET is specified CX_API_KEY/CX_PRIVATE_KEY shouldn't be specified.")]
    KeySourceConflict(),
    #[error("Either CX_SECRET or CX_API_KEY should be specified.")]
    MissingKeySource(),
    #[error("Invalid log mode name: '{name}'")]
    InvalidLogMode { name: String },
    #[error("Invalid metrics mode name: '{name}'")]
    InvalidPlatformEventLog { name: String },
    #[error("Invalid platform event log: '{name}'")]
    InvalidMetricsMode { name: String },
    #[error("Invalid OTEL metrics mode name: '{name}'")]
    InvalidOtelMetricsMode { name: String },
    #[error("Invalid tracing mode name: '{name}'")]
    InvalidTracingMode { name: String },
    #[error("Invalid trace sampling mode name: '{name}'")]
    InvalidTraceSamplingMode { name: String },
    #[error("Invalid built-in attribute name: '{name}'")]
    InvalidBuiltInAttribute { name: String },
    #[error(
        "Invalid span attribute exclusion: '{value}'. Expected either 'disabled', 'predefined' or a valid regex"
    )]
    InvalidAttributeExclusionMode { value: String },
    #[error("When CX_METRICS_MODE is specified CX_METRICS_ENABLED shouldn't be specified.")]
    MetricsModeConflict(),
    #[error(
        "When CX_LOG_ONLY is specified neither CX_METRICS_ENABLED nor CX_METRICS_ENABLED shouldn't be specified."
    )]
    LogOnlyMetricsConflict(),
    #[error("When CX_LOG_ONLY is specified CX_TRACING_MODE shouldn't be specified.")]
    LogOnlyTracingConflict(),
    #[error("When CX_LOG_ONLY is specified CX_LOG_MODE shouldn't be specified.")]
    LogModeLogConflict(),
    #[error("Invalid reporting strategy name: '{name}'")]
    InvalidReportingStrategy { name: String },
    #[error(
        "Invalid aws_telemetry_interval_ms: {value}. Only values between 25 and 30000 are accepted."
    )]
    InvalidTelemetryInterval { value: usize },
}

impl std::fmt::Debug for AppConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}
