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

use std::collections::HashMap;

use crate::config::app_config::{BuiltInAttributeSet, LogsMetadataMode, TargetConfigs};

use super::app_config::KeySourceConfig;
use super::app_config::TelemetryReportingStrategy::{LowOverhead, ReportAfterInvocation};
use super::app_config::{
    AppConfig, AttributeExclusionMode, OtlpTargetConfig, PlatformMetricsMode, TargetConfig,
    TraceSamplingMode, TracingMode,
};

fn coralogix_target() -> TargetConfigs {
    let cx = OtlpTargetConfig::Coralogix {
        domain: "coralogix.com".to_owned(),
        key_source_config: KeySourceConfig::EnvVar {
            key: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".into(),
        },
    };
    TargetConfigs {
        logs: None,
        traces: None,
        metrics: None,
        main: Some(TargetConfig::Otlp { target: cx }),
        alpn_enabled: false,
        combined_telemetry_enabled: false,
    }
}

#[test]
fn loading_config_from_env_vars() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_APPLICATION", Some("application")),
            ("CX_SUBSYSTEM", Some("subsystem")),
            ("CX_SERVICE_NAME", Some("service_name")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
            ("CX_AWS_TELEMETRY_INTERVAL_MS", Some("1000")),
            ("CX_METRICS_ENABLED", Some("false")),
            ("CX_LOGS_METADATA_ENABLED", Some("false")),
            ("CX_OTLP_SERVER_ENABLED", Some("true")),
            ("CX_TRACING_MODE", Some("otel")),
            ("CX_TRACE_SAMPLING_MODE", Some("follow_xray")),
            ("CX_TAGS_ENABLED", Some("true")),
            ("CX_TAG_CACHE_VALIDITY_MS", Some("1000")),
            ("CX_MESSAGE_SIZE_LIMIT", Some("1000")),
        ],
        || {
            let app = AppConfig::load().expect("should succeed");
            let context = &app.function_context_provider_config;
            let service = &app.telemetry_service_config;
            let processor = &app.telemetry_service_config.processor_config;
            assert_eq!(app.target, coralogix_target());
            assert_eq!(
                context.configured_application,
                Some("application".to_owned())
            );
            assert_eq!(context.configured_subsystem, Some("subsystem".to_owned()));
            assert_eq!(
                context.configured_service_name,
                Some("service_name".to_owned())
            );
            assert_eq!(service.reporting_strategy, LowOverhead);
            assert_eq!(app.aws_telemetry_interval_ms, 1000);
            assert_eq!(
                processor.platform_metrics_mode,
                PlatformMetricsMode::Disabled
            );
            assert_eq!(processor.logs_metadata_mode, LogsMetadataMode::Disabled);
            assert!(app.otlp_server_enabled);
            assert_eq!(processor.tracing_mode, TracingMode::OtelInstrumentation);
            assert_eq!(processor.trace_sampling_mode, TraceSamplingMode::FollowXray);
            assert!(app.tags_enabled);
            assert_eq!(
                context.tag_cache_validity,
                time::Duration::milliseconds(1000)
            );
            assert_eq!(processor.message_size_limit, 1000);
        },
    );
}

#[test]
fn user_friendly_name_in_error() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_APPLICATION", Some("application")),
            ("CX_SUBSYSTEM", Some("subsystem")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", None),
            ("CX_AWS_TELEMETRY_INTERVAL_MS", Some("1000")),
        ],
        || {
            let error = AppConfig::load().expect_err("should fail");
            assert_eq!(
                format!("{error:?}"), // when main crashes Debug is used to print the error to stdout
                "Environment variable CX_REPORTING_STRATEGY is missing".to_owned()
            )
        },
    );
}

#[test]
fn old_env_var_prefix() {
    temp_env::with_vars(
        vec![
            (
                "CORALOGIX_PRIVATE_KEY",
                Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
            ),
            ("CORALOGIX_DOMAIN", Some("coralogix.com")),
            ("CORALOGIX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            assert_eq!(c.target, coralogix_target());
            assert_eq!(c.telemetry_service_config.reporting_strategy, LowOverhead);
        },
    )
}

#[test]
fn old_private_key_can_be_used_instead_of_api_key() {
    temp_env::with_vars(
        vec![
            (
                "CX_PRIVATE_KEY",
                Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
            ),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            assert_eq!(c.target, coralogix_target());
            assert_eq!(c.telemetry_service_config.reporting_strategy, LowOverhead);
        },
    )
}

#[test]
fn otel_url_can_be_used_instead_of_domain() {
    temp_env::with_vars(
        vec![
            ("CX_OTEL_URL", Some("https://test.example.com:4444")),
            ("CX_REPORTING_STRATEGY", Some("REPORT_AFTER_INVOCATION")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            assert_eq!(
                c.target,
                TargetConfigs {
                    logs: None,
                    traces: None,
                    metrics: None,
                    main: Some(TargetConfig::Otlp {
                        target: OtlpTargetConfig::Otel {
                            url: "https://test.example.com:4444".to_owned()
                        }
                    }),
                    alpn_enabled: false,
                    combined_telemetry_enabled: false,
                }
            );
            assert_eq!(
                c.telemetry_service_config.reporting_strategy,
                ReportAfterInvocation
            );
        },
    )
}

#[test]
fn domain_can_be_mixed_with_otel_pillar_url() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_OTEL_TRACES_URL", Some("https://test.example.com:4444")),
            ("CX_REPORTING_STRATEGY", Some("REPORT_AFTER_INVOCATION")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            assert_eq!(
                c.target,
                TargetConfigs {
                    logs: None,
                    traces: Some(OtlpTargetConfig::Otel {
                        url: "https://test.example.com:4444".to_owned()
                    }),
                    metrics: None,
                    main: Some(TargetConfig::Otlp {
                        target: OtlpTargetConfig::Coralogix {
                            domain: "coralogix.com".to_owned(),
                            key_source_config: KeySourceConfig::EnvVar {
                                key: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".into(),
                            },
                        }
                    }),
                    alpn_enabled: false,
                    combined_telemetry_enabled: false,
                }
            );
            assert_eq!(
                c.telemetry_service_config.reporting_strategy,
                ReportAfterInvocation
            );
        },
    )
}

#[test]
fn firehose_can_be_used_instead_of_domain() {
    temp_env::with_vars(
        vec![
            ("CX_FIREHOSE", Some("my-delivery-stream")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            assert_eq!(
                c.target,
                TargetConfigs {
                    logs: None,
                    traces: None,
                    metrics: None,
                    main: Some(TargetConfig::Firehose {
                        delivery_stream_name: "my-delivery-stream".to_owned(),
                    }),
                    alpn_enabled: false,
                    combined_telemetry_enabled: false,
                }
            );
            assert_eq!(c.telemetry_service_config.reporting_strategy, LowOverhead);
        },
    )
}

#[test]
fn firehose_can_be_mixed_with_otel_pillar_url() {
    temp_env::with_vars(
        vec![
            ("CX_FIREHOSE", Some("my-delivery-stream")),
            ("CX_OTEL_TRACES_URL", Some("https://test.example.com:4444")),
            ("CX_REPORTING_STRATEGY", Some("REPORT_AFTER_INVOCATION")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            assert_eq!(
                c.target,
                TargetConfigs {
                    logs: None,
                    traces: Some(OtlpTargetConfig::Otel {
                        url: "https://test.example.com:4444".to_owned()
                    }),
                    metrics: None,
                    main: Some(TargetConfig::Firehose {
                        delivery_stream_name: "my-delivery-stream".to_owned(),
                    }),
                    alpn_enabled: false,
                    combined_telemetry_enabled: false,
                }
            );
            assert_eq!(
                c.telemetry_service_config.reporting_strategy,
                ReportAfterInvocation
            );
        },
    )
}

#[test]
fn excluded_span_attributes_accepts_regex() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
            ("CX_EXCLUDED_SPAN_ATTRIBUTES", Some(r"aaa\.bbb\..+|bbb.+")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            match c
                .telemetry_service_config
                .processor_config
                .excluded_span_attributes
            {
                AttributeExclusionMode::Regex(regex) => assert!(regex.is_match("aaa.bbb.ccc")),
                _ => panic!("wrong AttributeExclusionMode"),
            }
        },
    )
}

#[test]
fn excluded_span_attributes_rejects_invalid_regex() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
            ("CX_EXCLUDED_SPAN_ATTRIBUTES", Some("[abc+")),
        ],
        || {
            let error = AppConfig::load().expect_err("should fail");
            assert_eq!(
                format!("{error:?}"), // when main crashes Debug is used to print the error to stdout
                "Invalid span attribute exclusion: '[abc+'. Expected either 'disabled', 'predefined' or a valid regex"
                    .to_owned()
            )
        },
    )
}

#[test]
fn platform_event_logs_are_enabled_by_default() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            let logs = c.telemetry_service_config.processor_config.platform_log_set;

            assert!(logs.start);
            assert!(logs.runtime_done);
            assert!(logs.report);
        },
    )
}

#[test]
fn platform_event_logs_can_be_disabled() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
            ("CX_PLATFORM_LOGS", Some("disabled")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            let logs = c.telemetry_service_config.processor_config.platform_log_set;

            assert!(!logs.start);
            assert!(!logs.runtime_done);
            assert!(!logs.report);
        },
    )
}

#[test]
fn platform_event_logs_can_be_enabled_selectively() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
            ("CX_PLATFORM_LOGS", Some("  runtIme_dOne,   3  ")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            let logs = c.telemetry_service_config.processor_config.platform_log_set;

            assert!(!logs.start);
            assert!(logs.runtime_done);
            assert!(logs.report);
        },
    )
}

#[test]
fn resource_attributes_can_be_configured_for_all_pillars() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
            (
                "CX_RESOURCE_BUILT_IN_ATTRIBUTES",
                Some(" faas_id, faas_NaMe,cloud.region, "),
            ),
            (
                "CX_RESOURCE_EXTRA_ATTRIBUTES",
                Some(" ke.y1=val1 ,  KEY2=VAL2 , "),
            ),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            let attr = c
                .telemetry_service_config
                .processor_config
                .resource_attributes;

            assert_eq!(
                attr.logs.extra,
                HashMap::from([
                    ("ke.y1".to_owned(), "val1".to_owned()),
                    ("KEY2".to_owned(), "VAL2".to_owned()),
                ])
            );
            // The same extra attributes are set for all 3 pillars. This is not true for built-in attributes, as there are special restrictions per pillar.
            assert_eq!(attr.logs.extra, attr.metrics.extra);
            assert_eq!(attr.logs.extra, attr.traces.extra);

            assert_eq!(
                attr.logs.built_in,
                BuiltInAttributeSet {
                    faas_id: true,
                    faas_name: true,
                    cloud_region: true,
                    ..BuiltInAttributeSet::empty()
                }
            );
            assert_eq!(
                attr.traces.built_in,
                BuiltInAttributeSet {
                    faas_id: true,
                    faas_name: true,
                    cloud_region: true,
                    service_name: true, // Service name is always enabled for spans
                    ..BuiltInAttributeSet::empty()
                }
            );
            assert_eq!(
                attr.metrics.built_in,
                BuiltInAttributeSet {
                    faas_id: true,
                    faas_name: true,
                    cloud_region: true,
                    faas_instance_cx_id: true, // faas_instance_cx_id is always enabled for metrics
                    ..BuiltInAttributeSet::empty()
                }
            );
        },
    )
}

#[test]
fn resource_attributes_can_be_configured_for_specific_pillar() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
            ("CX_LOGS_RESOURCE_BUILT_IN_ATTRIBUTES", Some("faas_id")),
            ("CX_LOGS_RESOURCE_EXTRA_ATTRIBUTES", Some("key1=val1")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            let attr = c
                .telemetry_service_config
                .processor_config
                .resource_attributes;

            assert_eq!(
                attr.logs.extra,
                HashMap::from([("key1".to_owned(), "val1".to_owned())])
            );
            assert_eq!(attr.metrics.extra, HashMap::new());
            assert_eq!(attr.traces.extra, HashMap::new());

            assert_eq!(
                attr.logs.built_in,
                BuiltInAttributeSet {
                    faas_id: true,
                    ..BuiltInAttributeSet::empty()
                }
            );
            assert_eq!(attr.traces.built_in, BuiltInAttributeSet::all());
            assert_eq!(attr.metrics.built_in, BuiltInAttributeSet::all());
        },
    )
}

#[test]
fn pillar_attributes_overwrite_general_attributes() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
            ("CX_RESOURCE_BUILT_IN_ATTRIBUTES", Some("faas_id")),
            ("CX_RESOURCE_EXTRA_ATTRIBUTES", Some("key1=val1")),
            (
                "CX_TRACES_RESOURCE_BUILT_IN_ATTRIBUTES",
                Some("faas_name, service_name"),
            ),
            ("CX_TRACES_RESOURCE_EXTRA_ATTRIBUTES", Some("key2=val2")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            let attr = c
                .telemetry_service_config
                .processor_config
                .resource_attributes;

            assert_eq!(
                attr.logs.extra,
                HashMap::from([("key1".to_owned(), "val1".to_owned())])
            );
            assert_eq!(attr.logs.extra, attr.metrics.extra);
            assert_eq!(
                attr.traces.extra,
                HashMap::from([("key2".to_owned(), "val2".to_owned())])
            );

            assert_eq!(
                attr.logs.built_in,
                BuiltInAttributeSet {
                    faas_id: true,
                    ..BuiltInAttributeSet::empty()
                }
            );
            assert_eq!(
                attr.traces.built_in,
                BuiltInAttributeSet {
                    faas_name: true,
                    service_name: true,
                    ..BuiltInAttributeSet::empty()
                }
            );
            assert_eq!(
                attr.metrics.built_in,
                BuiltInAttributeSet {
                    faas_id: true,
                    faas_instance_cx_id: true,
                    ..BuiltInAttributeSet::empty()
                }
            );
        },
    )
}

#[test]
fn resource_attributes_config_can_be_empty() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
            ("CX_RESOURCE_BUILT_IN_ATTRIBUTES", Some("   ")),
            ("CX_RESOURCE_EXTRA_ATTRIBUTES", Some("   ")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            let attr = c
                .telemetry_service_config
                .processor_config
                .resource_attributes;

            assert_eq!(attr.logs.extra, HashMap::new());
            assert_eq!(attr.logs.extra, attr.metrics.extra);
            assert_eq!(attr.logs.extra, attr.traces.extra);

            assert_eq!(attr.logs.built_in, BuiltInAttributeSet::empty());
            assert_eq!(
                attr.traces.built_in,
                BuiltInAttributeSet {
                    service_name: true, // Service name is always enabled for spans
                    ..BuiltInAttributeSet::empty()
                }
            );
            assert_eq!(
                attr.metrics.built_in,
                BuiltInAttributeSet {
                    faas_instance_cx_id: true, // faas_instance_cx_id is always enabled for metrics
                    ..BuiltInAttributeSet::empty()
                }
            );
        },
    )
}

#[test]
fn customize_logs_metadata() {
    temp_env::with_vars(
        vec![
            ("CX_API_KEY", Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")),
            ("CX_DOMAIN", Some("coralogix.com")),
            ("CX_REPORTING_STRATEGY", Some("LOW_OVERHEAD")),
            ("CX_LOGS_METADATA_INCLUDE_TRACE_REF", Some("false")),
            ("CX_LOGS_METADATA_INCLUDE_EXECUTION", Some("false")),
            ("CX_LOGS_METADATA_INCLUDE_INVOCATION_ID", Some("true")),
        ],
        || {
            let c = AppConfig::load().expect("should succeed");
            let LogsMetadataMode::Enabled(config) = c
                .telemetry_service_config
                .processor_config
                .logs_metadata_mode
            else {
                panic!("should be enabled");
            };

            assert!(!config.include_trace_ref);
            assert!(!config.include_execution);
            assert!(config.include_invocation_id);
        },
    )
}
