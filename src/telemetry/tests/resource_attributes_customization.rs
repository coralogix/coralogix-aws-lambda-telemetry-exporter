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

use std::collections::HashSet;

use crate::proto::opentelemetry::proto::common::v1::{AnyValue, KeyValue, any_value::Value};

use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn produce_telemetry_with_customized_resources() {
    // initialize_logging();
    let coralogix = Arc::new(CoralogixClientMock::default());
    let baseline_config = telemetry_service_config(
        TelemetryReportingStrategy::ReportAfterInvocation,
        TracingMode::TelemetryApi,
    );
    let config = TelemetryServiceConfig {
        processor_config: TelemetryProcessorConfig {
            resource_attributes: AttributeConfigs {
                logs: AttributeConfig {
                    built_in: BuiltInAttributeSet {
                        faas_id: true,
                        cloud_account_id: true,
                        ..BuiltInAttributeSet::empty()
                    },
                    extra: HashMap::from([("log_key".to_owned(), "log_value".to_owned())]),
                },
                traces: AttributeConfig {
                    built_in: BuiltInAttributeSet {
                        faas_name: true,
                        cloud_provider: true,
                        service_name: true,
                        ..BuiltInAttributeSet::empty()
                    },
                    extra: HashMap::from([("span_key".to_owned(), "span_value".to_owned())]),
                },
                metrics: AttributeConfig {
                    built_in: BuiltInAttributeSet {
                        faas_instance_cx_id: true,
                        cloud_region: true,
                        ..BuiltInAttributeSet::empty()
                    },
                    extra: HashMap::from([("metric_key".to_owned(), "metric_value".to_owned())]),
                },
            },
            ..baseline_config.processor_config
        },
        ..baseline_config
    };

    let ts = make_telemetry_service_with_config(
        function_context_provider(),
        Some(coralogix.clone()),
        config,
    );

    //// INVOCATION 1 ////
    let ready_to_freez = handle_invoke_event(&ts, REQUEST1_ID, TRACING1);
    give_events_time_to_process().await;

    ts.handle_incoming_telemetry(vec![
        init_start(),
        init_runtime_done(),
        init_report(),
        platform_start(REQUEST1_ID, &TRACE_CONTEXT1),
        platform_runtime_done(REQUEST1_ID, &TRACE_CONTEXT1),
    ])
    .await
    .unwrap();

    expect_readiness(ready_to_freez).await;

    //// INVOCATION 2 ////
    let ready_to_freez = handle_invoke_event(&ts, REQUEST2_ID, TRACING2);
    give_events_time_to_process().await;

    ts.handle_incoming_telemetry(vec![
        platform_report(REQUEST1_ID, &TRACE_CONTEXT1),
        platform_start(REQUEST2_ID, &TRACE_CONTEXT2),
        platform_runtime_done(REQUEST2_ID, &TRACE_CONTEXT2),
    ])
    .await
    .unwrap();

    expect_readiness(ready_to_freez).await;

    let log_resource = coralogix.resource_logs.lock().await[0]
        .resource
        .clone()
        .unwrap();
    assert_eq!(
        attribute_set(&log_resource.attributes),
        [
            ("cx.application.name", "200000000000"),
            ("cx.subsystem.name", "my-lambda"),
            (
                "faas.id",
                "arn:aws:lambda:eu-west-1:200000000000:function:my-lambda:7"
            ),
            ("cloud.account.id", "200000000000"),
            ("log_key", "log_value"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect::<HashSet<_, _>>()
    );

    let span_resource = coralogix.resource_spans.lock().await[0]
        .resource
        .clone()
        .unwrap();
    assert_eq!(
        attribute_set(&span_resource.attributes),
        [
            ("cx.application.name", "200000000000"),
            ("cx.subsystem.name", "my-lambda"),
            ("faas.name", "my-lambda"),
            ("cloud.provider", "aws"),
            ("service.name", "my-lambda"),
            ("span_key", "span_value"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect::<HashSet<_, _>>()
    );

    let metric_resource = coralogix.resource_metrics.lock().await[0]
        .resource
        .clone()
        .unwrap();
    assert_eq!(
        attribute_set(&metric_resource.attributes),
        [
            ("cx.application.name", "200000000000"),
            ("cx.subsystem.name", "my-lambda"),
            ("faas.instance.cx_id", "abcd"),
            ("cloud.region", "eu-west-1"),
            ("metric_key", "metric_value"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect::<HashSet<_, _>>()
    );
}

fn attribute_set(attributes: &[KeyValue]) -> HashSet<(String, String)> {
    attributes
        .iter()
        .map(|kv| (kv.key.clone(), as_string(&kv.value)))
        .collect()
}

fn as_string(value: &Option<AnyValue>) -> String {
    match value.as_ref().and_then(|v| v.value.as_ref()) {
        Some(Value::StringValue(s)) => s.clone(),
        _ => panic!("Expected string value, got {:?}", value),
    }
}
