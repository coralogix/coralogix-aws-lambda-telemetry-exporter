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

use super::log_maker::LogMakerConfig;
use super::{
    LogMetadata, PlatformEventLog, ReportLogMetrics, RuntimeDoneLogMetrics, is_out_of_memory,
    log_maker, span_id_to_string, trace_id_to_string,
};
use crate::Error;
use crate::config::app_config::{LogsMetadataMode, TelemetryProcessorConfig};
use crate::proto::opentelemetry::proto::logs::v1::{LogRecord, SeverityNumber};
use crate::telemetry::*;
use lambda_extension::Status;
use processor::{PlatformEventLogV2, ReportLogMetricsV2, RuntimeDoneLogMetricsV2};
use std::cmp::min;
use time::OffsetDateTime;

pub(super) fn make_function_log<S>(
    config: &TelemetryProcessorConfig,
    state: &mut S,
    log_text: String,
    time: OffsetDateTime,
) -> Result<LogRecord, Error>
where
    S: ProcessingState,
{
    log_maker::make_function_log_record(
        LogMakerConfig {
            message_size_limit: config.message_size_limit,
        },
        log_text,
        log_metadata(config, state),
        make_log_timestamp(state.last_log_state_mut(), time),
        state.current_span_ref(),
    )
}

pub(super) fn make_platform_start_log<S>(
    config: &TelemetryProcessorConfig,
    state: &mut S,
    start_data: PlatformStartData,
) -> Result<LogRecord, Error>
where
    S: ProcessingState,
{
    let request_id = if config.platform_logs.include_request_id {
        Some(start_data.request_id.clone())
    } else {
        None
    };
    if config.platform_logs.hide_default_values {
        let event = PlatformEventLogV2::Start {
            request_id,
            version: start_data.version,
        };
        log_maker::make_platform_log_record_v2(
            event,
            SeverityNumber::Info,
            log_metadata(config, state),
            make_log_timestamp(state.last_log_state_mut(), start_data.time),
            state.current_span_ref(),
        )
    } else {
        let event = PlatformEventLog::Start {
            request_id,
            version: start_data.version,
        };
        log_maker::make_platform_log_record(
            event,
            SeverityNumber::Info,
            log_metadata(config, state),
            make_log_timestamp(state.last_log_state_mut(), start_data.time),
            state.current_span_ref(),
        )
    }
}

pub(super) fn make_platform_runtime_done_log<S>(
    config: &TelemetryProcessorConfig,
    state: &mut S,
    runtime_done_data: &PlatformRuntimeDoneData,
    handler_status: Option<String>,
    handler_status_description: Option<String>,
) -> Result<LogRecord, Error>
where
    S: ProcessingState,
{
    let request_id = if config.platform_logs.include_request_id {
        Some(runtime_done_data.request_id.clone())
    } else {
        None
    };
    if config.platform_logs.hide_default_values {
        let event = PlatformEventLogV2::RuntimeDone {
            request_id,
            aws_status: status_to_string(&runtime_done_data.status),
            aws_error_type: runtime_done_data.error_type.clone(),
            handler_status,
            handler_status_description,
            metrics: RuntimeDoneLogMetricsV2 {
                duration_ms: runtime_done_data.metrics.as_ref().map(|x| x.duration_ms),
                produced_bytes: runtime_done_data
                    .metrics
                    .as_ref()
                    .and_then(|x| x.produced_bytes),
            },
        };
        let severity = status_to_severity(&runtime_done_data.status);
        log_maker::make_platform_log_record_v2(
            event,
            severity,
            log_metadata(config, state),
            make_log_timestamp(state.last_log_state_mut(), runtime_done_data.time),
            state.current_span_ref(),
        )
    } else {
        let event = PlatformEventLog::RuntimeDone {
            request_id,
            aws_status: status_to_string(&runtime_done_data.status),
            aws_error_type: runtime_done_data.error_type.clone(),
            handler_status,
            handler_status_description,
            metrics: RuntimeDoneLogMetrics {
                duration_ms: runtime_done_data.metrics.as_ref().map(|x| x.duration_ms),
                produced_bytes: runtime_done_data
                    .metrics
                    .as_ref()
                    .and_then(|x| x.produced_bytes),
            },
        };
        let severity = status_to_severity(&runtime_done_data.status);
        log_maker::make_platform_log_record(
            event,
            severity,
            log_metadata(config, state),
            make_log_timestamp(state.last_log_state_mut(), runtime_done_data.time),
            state.current_span_ref(),
        )
    }
}

pub(super) fn make_platform_report_log<S>(
    config: &TelemetryProcessorConfig,
    state: &mut S,
    report_data: &PlatformReportData,
) -> Result<LogRecord, Error>
where
    S: ProcessingState,
{
    let out_of_memory = is_out_of_memory(report_data);
    let request_id = if config.platform_logs.include_request_id {
        Some(report_data.request_id.clone())
    } else {
        None
    };
    let severity = status_to_severity(&report_data.status);
    if config.platform_logs.hide_default_values {
        let event = PlatformEventLogV2::Report {
            request_id,
            status: status_to_string(&report_data.status),
            error_type: report_data.error_type.clone(),
            out_of_memory,
            metrics: ReportLogMetricsV2 {
                duration_ms: report_data.metrics.duration_ms,
                billed_duration_ms: report_data.metrics.billed_duration_ms,
                memory_size_mb: report_data.metrics.memory_size_mb,
                max_memory_used_mb: report_data.metrics.max_memory_used_mb,
                init_duration_ms: report_data.metrics.init_duration_ms,
                restore_duration_ms: report_data.metrics.restore_duration_ms,
                produced_bytes: state
                    .runtime_done_data()
                    .as_ref()
                    .and_then(|x| x.metrics.as_ref())
                    .and_then(|x| x.produced_bytes),
            },
        };
        log_maker::make_platform_log_record_v2(
            event,
            severity,
            log_metadata(config, state),
            make_log_timestamp(state.last_log_state_mut(), report_data.time),
            state.current_span_ref(),
        )
    } else {
        let event = PlatformEventLog::Report {
            request_id,
            status: status_to_string(&report_data.status),
            error_type: report_data.error_type.clone(),
            out_of_memory,
            metrics: ReportLogMetrics {
                duration_ms: report_data.metrics.duration_ms,
                billed_duration_ms: report_data.metrics.billed_duration_ms,
                memory_size_mb: report_data.metrics.memory_size_mb,
                max_memory_used_mb: report_data.metrics.max_memory_used_mb,
                init_duration_ms: report_data.metrics.init_duration_ms,
                restore_duration_ms: report_data.metrics.restore_duration_ms,
                produced_bytes: state
                    .runtime_done_data()
                    .as_ref()
                    .and_then(|x| x.metrics.as_ref())
                    .and_then(|x| x.produced_bytes),
            },
        };
        log_maker::make_platform_log_record(
            event,
            severity,
            log_metadata(config, state),
            make_log_timestamp(state.last_log_state_mut(), report_data.time),
            state.current_span_ref(),
        )
    }
}

fn log_metadata<S>(config: &TelemetryProcessorConfig, state: &S) -> Option<LogMetadata>
where
    S: ProcessingState,
{
    match &config.logs_metadata_mode {
        LogsMetadataMode::Disabled => None,
        LogsMetadataMode::Enabled(c) => {
            let span_ref = state.current_span_ref();
            let trace_id = c
                .include_trace_ref
                .then(|| trace_id_to_string(&span_ref.trace_id));
            let span_id = c
                .include_trace_ref
                .then(|| span_id_to_string(&span_ref.span_id));
            let faas_execution = c.include_execution.then(|| state.request_id()).flatten();
            let faas_invocation_id = c
                .include_invocation_id
                .then(|| state.request_id())
                .flatten();

            let built_in = &config.resource_attributes.logs.built_in;
            let function_context = state.function_context();
            let arn = &function_context.arn;
            let cloud_provider = built_in.cloud_provider.then(|| "aws".to_owned());
            let cloud_account_id = built_in.cloud_account_id.then(|| arn.account_id.clone());
            let cloud_region = built_in.cloud_region.then(|| arn.region.clone());
            let faas_name = built_in.faas_name.then(|| arn.function_name.clone());
            let faas_id = built_in
                .faas_id
                .then(|| function_context.version_arn.to_string());
            let faas_instance_cx_id = built_in
                .faas_instance_cx_id
                .then(|| function_context.lambda_instance_coralogix_id.clone());

            let extra_attributes = config.resource_attributes.logs.extra.clone();

            Some(LogMetadata {
                trace_id,
                span_id,
                cloud_provider,
                cloud_account_id,
                cloud_region,
                faas_name,
                faas_id,
                faas_instance_cx_id,
                faas_execution,
                faas_invocation_id,
                extra_attributes,
                tags: function_context.tags.clone(),
            })
        }
    }
}

fn status_to_string(status: &Status) -> String {
    match status {
        Status::Success => "success".to_owned(),
        Status::Error => "error".to_owned(),
        Status::Failure => "failure".to_owned(),
        Status::Timeout => "timeout".to_owned(),
    }
}

fn status_to_severity(status: &Status) -> SeverityNumber {
    if *status == Status::Success {
        SeverityNumber::Info
    } else {
        SeverityNumber::Error
    }
}

// AWS Telemetry API provides timestamps with millisecond resolution.
// When multiple logs are emitted in a millisecond, the order of these logs get messed up in Coralogix as they are sorted by the timestamp.
// In order to preserve the order of logs, we synthesize the nanoseconds part of the timestamp, so that subsequent logs within one millisecond have subsequent timestamps.
fn make_log_timestamp(state: &mut LastLogState, timestamp: OffsetDateTime) -> u64 {
    let nanos = timestamp.unix_timestamp_nanos();
    let nanos_u64 = if nanos < 0 {
        0
    } else if nanos > u64::MAX as i128 {
        u64::MAX
    } else {
        nanos as u64
    };

    let subindex = if timestamp == state.last_log_timestamp {
        state.last_log_subindex += 1;
        // We don't want the subindex treated as nanoseconds to influence the milliseconds part of the timestamp
        min(state.last_log_subindex, 999999)
    } else {
        state.last_log_timestamp = timestamp;
        state.last_log_subindex = 0;
        state.last_log_subindex
    };

    nanos_u64 + subindex as u64
}
