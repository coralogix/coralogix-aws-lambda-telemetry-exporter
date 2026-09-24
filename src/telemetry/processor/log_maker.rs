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

use crate::proto::opentelemetry::proto::common::v1::AnyValue;
use crate::proto::opentelemetry::proto::logs::v1::{LogRecord, SeverityNumber};
use crate::telemetry::SpanRef;
use lambda_extension::Error;
use serde::Serialize;

use super::{LogMetadata, PlatformEventLog, PlatformEventLogV2};

pub struct LogMakerConfig {
    pub message_size_limit: usize,
}

#[derive(Debug, Clone, Serialize)]
struct TextLogWithMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    cx_metadata: Option<LogMetadata>,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct JsonLogWithMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    cx_metadata: Option<LogMetadata>,
    #[serde(flatten)]
    json_payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
struct PlatformEventLogWithMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    cx_metadata: Option<LogMetadata>,
    #[serde(flatten)]
    event: PlatformEventLog,
}

#[derive(Debug, Clone, Serialize)]
struct PlatformEventLogV2WithMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    cx_metadata: Option<LogMetadata>,
    #[serde(flatten)]
    event: PlatformEventLogV2,
}

pub(super) fn make_function_log_record(
    config: LogMakerConfig,
    log_text: String,
    metadata: Option<LogMetadata>,
    timestamp: u64,
    span_ref: SpanRef,
) -> Result<LogRecord, Error> {
    let log_text = truncate_with_annotation_if_too_long(log_text, config.message_size_limit);
    let severity = infer_severity(&log_text);
    let body = match serde_json::from_str::<serde_json::Value>(log_text.as_str()) {
        Ok(serde_json::Value::Object(json)) => {
            AnyValue::from(serde_json::to_value(JsonLogWithMetadata {
                cx_metadata: metadata,
                json_payload: serde_json::Value::Object(json),
            })?)
        }
        _ => AnyValue::from(serde_json::to_value(TextLogWithMetadata {
            cx_metadata: metadata,
            message: log_text,
        })?),
    };

    Ok(LogRecord {
        time_unix_nano: timestamp,
        observed_time_unix_nano: 0, // logs-gateway doesn't care
        severity_number: severity as i32,
        severity_text: "".to_owned(), // logs-gateway doesn't care
        body: Some(body),
        attributes: Vec::new(),
        dropped_attributes_count: 0,
        flags: 0,
        trace_id: span_ref.trace_id,
        span_id: span_ref.span_id,
    })
}

fn truncate_with_annotation_if_too_long(log_text: String, message_size_limit: usize) -> String {
    let overflow = log_text.len() as i64 - message_size_limit as i64;
    if overflow <= 0 {
        log_text
    } else if overflow < 1000 {
        format!(
            "{}...({}B truncated)...",
            truncate(log_text.as_str(), message_size_limit),
            overflow
        )
    } else {
        format!(
            "{}...({}kB truncated)...",
            truncate(log_text.as_str(), message_size_limit),
            overflow / 1000
        )
    }
}

fn truncate(s: &str, mut n: usize) -> &str {
    if n >= s.len() {
        s
    } else {
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        &s[..n]
    }
}

pub(super) fn make_platform_log_record(
    event: PlatformEventLog,
    severity: SeverityNumber,
    metadata: Option<LogMetadata>,
    timestamp: u64,
    span_ref: SpanRef,
) -> Result<LogRecord, Error> {
    let body = AnyValue::from(serde_json::to_value(PlatformEventLogWithMetadata {
        cx_metadata: metadata,
        event,
    })?);
    Ok(LogRecord {
        time_unix_nano: timestamp,
        observed_time_unix_nano: 0, // logs-gateway doesn't care
        severity_number: severity as i32,
        severity_text: "".to_owned(), // logs-gateway doesn't care
        body: Some(body),
        attributes: Vec::new(),
        dropped_attributes_count: 0,
        flags: 0,
        trace_id: span_ref.trace_id,
        span_id: span_ref.span_id,
    })
}

pub(super) fn make_platform_log_record_v2(
    event: PlatformEventLogV2,
    severity: SeverityNumber,
    metadata: Option<LogMetadata>,
    timestamp: u64,
    span_ref: SpanRef,
) -> Result<LogRecord, Error> {
    let body = AnyValue::from(serde_json::to_value(PlatformEventLogV2WithMetadata {
        cx_metadata: metadata,
        event,
    })?);
    Ok(LogRecord {
        time_unix_nano: timestamp,
        observed_time_unix_nano: 0, // logs-gateway doesn't care
        severity_number: severity as i32,
        severity_text: "".to_owned(), // logs-gateway doesn't care
        body: Some(body),
        attributes: Vec::new(),
        dropped_attributes_count: 0,
        flags: 0,
        trace_id: span_ref.trace_id,
        span_id: span_ref.span_id,
    })
}

// Inspired by https://github.com/coralogix/aws-lambda-extension/blob/master/pkg/coralogixapiclient/helpers.go#L18
fn infer_severity(log_text: &str) -> SeverityNumber {
    let t = log_text.to_lowercase();

    // This isn't perfectly OTLP
    if t.contains("fatal") || t.contains("critical") {
        SeverityNumber::Fatal
    } else if t.contains("error") || t.contains("exception") {
        SeverityNumber::Error
    } else if t.contains("warn") {
        SeverityNumber::Warn
    } else if t.contains("verbose") || t.contains("trace") {
        SeverityNumber::Trace
    } else if t.contains("debug") {
        SeverityNumber::Debug
    } else {
        SeverityNumber::Info
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{make_function_log_record, make_platform_log_record};
    use crate::proto::opentelemetry::proto::common::v1::AnyValue;
    use crate::proto::opentelemetry::proto::logs::v1::SeverityNumber;
    use crate::telemetry::SpanRef;
    use crate::telemetry::processor::log_maker::{LogMakerConfig, LogMetadata, PlatformEventLog};

    const TRACE_ID: Vec<u8> = Vec::new();
    const SPAN_ID: Vec<u8> = Vec::new();
    const SPAN_REF: SpanRef = SpanRef {
        trace_id: TRACE_ID,
        span_id: SPAN_ID,
    };
    const CONFIG: LogMakerConfig = LogMakerConfig {
        message_size_limit: 30_000,
    };

    fn any_value(s: &str) -> AnyValue {
        let json_value = serde_json::from_str::<serde_json::Value>(s).unwrap();
        AnyValue::from(json_value)
    }

    #[test]
    fn json_function_log() {
        let log_record = make_function_log_record(
            CONFIG,
            r#"{"field1": "a", "field2": "b"}"#.to_owned(),
            None,
            1,
            SPAN_REF,
        )
        .unwrap();

        assert_eq!(
            log_record.body.unwrap(),
            any_value(r#"{"field1":"a","field2":"b"}"#)
        )
    }

    // anything that is a valid JSON value but not a JSON object should be treated as text
    #[test]
    fn number_function_log() {
        let log_record =
            make_function_log_record(CONFIG, r#"1"#.to_owned(), None, 1, SPAN_REF).unwrap();

        assert_eq!(log_record.body.unwrap(), any_value(r#"{"message":"1"}"#))
    }

    #[test]
    fn text_function_log() {
        let log_record =
            make_function_log_record(CONFIG, r#"aaa"#.to_owned(), None, 1, SPAN_REF).unwrap();

        assert_eq!(log_record.body.unwrap(), any_value(r#"{"message":"aaa"}"#))
    }

    #[test]
    fn platform_log() {
        let log_record = make_platform_log_record(
            PlatformEventLog::Start {
                request_id: Some("aaa".to_owned()),
                version: Some("123".to_owned()),
            },
            SeverityNumber::Info,
            None,
            1,
            SPAN_REF,
        )
        .unwrap();

        assert_eq!(
            log_record.body.unwrap(),
            any_value(
                r#"{"event":{"request_id": "aaa", "version": "123"}, "platform_event_type": "start"}"#
            )
        )
    }

    #[test]
    fn with_metadata() {
        let log_record = make_function_log_record(
            CONFIG,
            r#"aaa"#.to_owned(),
            Some(LogMetadata {
                trace_id: Some("00000000000000000000000000000000".to_owned()),
                span_id: Some("0000000000000000".to_owned()),
                cloud_provider: Some("aws".to_owned()),
                cloud_account_id: Some("200000000000".to_owned()),
                cloud_region: Some("eu-west-1".to_owned()),
                faas_name: Some("lambda-telemetry-test".to_owned()),
                faas_id: Some(
                    "arn:aws:lambda:eu-west-1:200000000000:function:lambda-telemetry-test:1"
                        .to_owned(),
                ),
                faas_instance_cx_id: Some("00000000000000000000000000000000".to_owned()),
                faas_execution: Some("ffffffff-0000-0000-0000-000000000000".to_owned()),
                faas_invocation_id: Some("ffffffff-0000-0000-0000-000000000000".to_owned()),
                extra_attributes: HashMap::from([(
                    "extra_attribute".to_owned(),
                    "value".to_owned(),
                )]),
                tags: Some(HashMap::from([(
                    "tag_name1".to_owned(),
                    "tag_value1".to_owned(),
                )])),
            }),
            1,
            SPAN_REF,
        )
        .unwrap();

        assert_eq!(
            log_record.body.unwrap(),
            any_value(
                r#"{"cx_metadata":{"trace_id":"00000000000000000000000000000000","span_id":"0000000000000000","cloud_provider":"aws","cloud_account_id":"200000000000","cloud_region":"eu-west-1","faas_name":"lambda-telemetry-test","faas_id":"arn:aws:lambda:eu-west-1:200000000000:function:lambda-telemetry-test:1","faas_instance_cx_id":"00000000000000000000000000000000","faas_execution":"ffffffff-0000-0000-0000-000000000000","faas_invocation_id":"ffffffff-0000-0000-0000-000000000000","extra_attribute":"value","tags":{"tag_name1":"tag_value1"}},"message":"aaa"}"#
            )
        )
    }

    #[test]
    fn truncate_message() {
        let log_record = make_function_log_record(
            LogMakerConfig {
                message_size_limit: 10,
            },
            r#"this stays| this should be dropped"#.to_owned(),
            None,
            1,
            SPAN_REF,
        )
        .unwrap();

        assert_eq!(
            log_record.body.unwrap(),
            any_value(r#"{"message":"this stays...(24B truncated)..."}"#)
        )
    }

    #[test]
    fn truncate_message_with_multi_byte_unicode_characters() {
        let log_record = make_function_log_record(
            LogMakerConfig {
                message_size_limit: 10,
            },
            r#"ラムダテレメトリー"#.to_owned(),
            None,
            1,
            SPAN_REF,
        )
        .unwrap();

        assert_eq!(
            log_record.body.unwrap(),
            any_value(r#"{"message":"ラムダ...(17B truncated)..."}"#)
        )
    }
}
