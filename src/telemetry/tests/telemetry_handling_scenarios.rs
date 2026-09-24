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
use crate::proto::opentelemetry::proto::trace::v1::status::StatusCode;
use std::sync::Arc;

// TODO check if logs are correctly assigned to invocations

// This is a reproduction of issue that ocurred when lambda instance was reinitialized (due to prior failure), and the freshly initialized telemetry_exporter received `report` (from the previous invocation) event before invoke event, and then immediately concluded that that invocation is complete.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handling_report_after_crash_before_next_invocation() {
    // initialize_logging();
    let coralogix = Arc::new(CoralogixClientMock::default());
    let ts = make_telemetry_service(function_context_provider(), Some(coralogix.clone()));

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

    //// At this point (or a bit earlier) the lambda environment crashes (for whatever reason) and is reinitialized, so the telemetry_service starts with a fresh state.

    let ts = make_telemetry_service(function_context_provider(), Some(coralogix.clone()));

    // A report from previous invocation (request1)
    // Because this telemetry_service doesn't know anything about request1, it may misinterpret this report as a conclusion of request2.
    ts.handle_incoming_telemetry(vec![platform_report(REQUEST1_ID, &TRACE_CONTEXT1)])
        .await
        .unwrap();

    //// INVOCATION 2 ////
    let ready_to_freez = handle_invoke_event(&ts, REQUEST2_ID, TRACING2);
    give_events_time_to_process().await;

    ts.handle_incoming_telemetry(vec![
        init_start(),
        init_runtime_done(),
        init_report(),
        platform_start(REQUEST2_ID, &TRACE_CONTEXT2),
        platform_runtime_done(REQUEST2_ID, &TRACE_CONTEXT2),
    ])
    .await
    .unwrap();

    expect_readiness(ready_to_freez).await;

    // Metrics from the report from the first invocation are reported during the second invocation (in degraded mode)
    let metrics = extract_metrics(&coralogix).await;
    assert_eq!(metrics.len(), 6)
}

// This is very similar to handling_report_after_crash_before_next_invocation except there is more leftover telemetry.
// This scenario is based on something that happened to the rafal-telemetry-maybe-crash-on-invocation function
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handling_telemetry_after_crash_before_next_invocation() {
    // initialize_logging();

    // Lambda crashes during invocation1 before any telemetry events are emitted.
    // Then it is restarted and handles next invocation

    let coralogix = Arc::new(CoralogixClientMock::default());
    let ts = make_telemetry_service(function_context_provider(), Some(coralogix.clone()));

    // A complete telemetry from previous invocation (request1)
    // Because this telemetry_service doesn't know anything about request1, it may misinterpret this report as a conclusion of request2.
    ts.handle_incoming_telemetry(vec![
        init_start(),
        function_log("Log from previous init"),
        init_runtime_done(),
        init_report(),
        platform_start(REQUEST1_ID, &TRACE_CONTEXT1),
        function_log("Log from request1"),
        platform_runtime_done(REQUEST1_ID, &TRACE_CONTEXT1),
        platform_report(REQUEST1_ID, &TRACE_CONTEXT1),
    ])
    .await
    .unwrap();

    ts.handle_incoming_telemetry(vec![init_start()])
        .await
        .unwrap();

    //// INVOCATION 2 ////
    let ready_to_freez = handle_invoke_event(&ts, REQUEST2_ID, TRACING2);
    give_events_time_to_process().await;

    ts.handle_incoming_telemetry(vec![
        function_log("Log from init"),
        init_runtime_done(),
        init_report(),
        platform_start(REQUEST2_ID, &TRACE_CONTEXT2),
        function_log("Log from request2"),
        platform_runtime_done(REQUEST2_ID, &TRACE_CONTEXT2),
    ])
    .await
    .unwrap();

    expect_readiness(ready_to_freez).await;

    // TODO write a test with similar assertions for OTEL tracing mode to ensure that spans have correct ids
    let logs = extract_log_records(&coralogix).await;
    assert_eq!(logs.len(), 9);
    assert_log_contains(&logs[0], "Log from previous init");
    assert_log_contains(&logs[1], "\"start\"");
    assert_log_contains(&logs[2], "Log from request1");
    assert_log_contains(&logs[3], "\"runtime_done\"");
    assert_log_contains(&logs[4], "\"report\"");

    assert_log_contains(&logs[5], "Log from init");
    assert_log_contains(&logs[6], "\"start\"");
    assert_log_contains(&logs[7], "Log from request2");
    assert_log_contains(&logs[8], "\"runtime_done\"");

    let (trace1, trace1_span1) = assert_belong_to_one_span(&logs[0..5]);
    let (trace2_1, trace2_span1) = assert_belong_to_one_span(&logs[5..6]);
    let (trace2_2, trace2_span2) = assert_belong_to_one_span(&logs[6..9]);

    assert_ne!(trace1, trace2_1);
    assert_eq!(trace2_1, trace2_2);

    assert_ne!(trace1_span1, trace2_span1);
    assert_ne!(trace1_span1, trace2_span2);
    assert_ne!(trace2_span1, trace2_span2);
}

// This is a reproduction of issue where enabling tags leads to telemetry exporter not signalizing `next` on short running lambdas
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn receiving_complete_telemetry_while_waiting_for_tags() {
    // initialize_logging();
    let ts = make_telemetry_service(function_context_provider_with_tags(), None);

    // first invoke event will lead to obtaining tags which will take 100ms
    let ready_to_freez = handle_invoke_event(&ts, REQUEST1_ID, TRACING1);

    // TODO test what happens when function telemetry is received here. (also test with low_overhead strategy)

    // The function finishes fast and the complete telemetry arrives before the invoke event is enqueue, so it cannot be processed immediately.
    ts.handle_incoming_telemetry(vec![
        init_start(),
        init_runtime_done(),
        init_report(),
        platform_start(REQUEST1_ID, &TRACE_CONTEXT1),
        platform_runtime_done(REQUEST1_ID, &TRACE_CONTEXT1),
    ])
    .await
    .unwrap();

    // Once the tags are obtained and the invoke event can be processed, the enqueued telemetry should be processed, leading to readiness to freez.
    expect_readiness(ready_to_freez).await;
}

// This is a reproduction of issue where shutdown happening before the first invocation has trouble sending telemetry because there is no function context.
// It was reproduced with the rafal-telemetry-python-failing-init lambda function
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handling_init_failure_followed_by_shutdown() {
    // initialize_logging();
    let coralogix = Arc::new(CoralogixClientMock::default());
    let ts = make_telemetry_service(function_context_provider(), Some(coralogix.clone()));

    // Some telemetry from the startup process is received. It doesn't matter what it is, but it matter that there will be something to send to coralogix
    ts.handle_incoming_telemetry(vec![init_start(), function_log("Init has failed!")])
        .await
        .unwrap();

    // The log received earlier should be sent now during shutdown, but there was no invocation, so we don't have the complete function context.
    let ready_to_shutdown = handle_shutdown_event(&ts);
    give_events_time_to_process().await;
    expect_readiness(ready_to_shutdown).await;

    // The failure log is actually delivered to coralogix
    let logs = extract_log_records(&coralogix).await;
    assert_eq!(logs.len(), 1);
    assert_log_contains(&logs[0], "Init has failed!");
}

// This is reproduction of something that happened to lambda-test-NodejsFailInitOtel-WfMYo2tn2Fz7
// https://eu-west-1.console.aws.amazon.com/cloudwatch/home?region=eu-west-1#logsV2:log-groups/log-group/$252Faws$252Flambda$252Flambda-test-NodejsFailInitOtel-WfMYo2tn2Fz7/log-events/2023$252F07$252F21$252F$255B$2524LATEST$255D57ad676146f549a1bf2a65e135c0507e
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handling_init_failure_followed_by_shutdown_and_telemetry_during_shutdown() {
    // initialize_logging();
    let coralogix = Arc::new(CoralogixClientMock::default());
    let ts = make_telemetry_service(function_context_provider(), Some(coralogix.clone()));

    // Some telemetry from the startup process is received.
    ts.handle_incoming_telemetry(vec![init_start(), function_log("Init has failed!")])
        .await
        .unwrap();

    // The log received earlier should be sent during shutdown
    let ready_to_shutdown = handle_shutdown_event(&ts);
    give_events_time_to_process().await;

    // While shutdown is in progress, a new batch of telemetry is received and it contains enough information to create an failed init span.
    ts.handle_incoming_telemetry(vec![
        function_log("Init has failed 2!"),
        init_runtime_done_failed(),
        init_report(),
        platform_start(REQUEST1_ID, &TRACE_CONTEXT1),
        platform_runtime_done(REQUEST1_ID, &TRACE_CONTEXT1),
    ])
    .await
    .unwrap();

    expect_readiness(ready_to_shutdown).await;

    // Both logs (+ platform logs) are delivered to coralogix
    let logs = extract_log_records(&coralogix).await;
    assert_eq!(logs.len(), 4);
    assert_log_contains(&logs[0], "Init has failed!");
    assert_log_contains(&logs[1], "Init has failed 2!");
    assert_log_contains(&logs[2], "\"start\"");
    assert_log_contains(&logs[3], "\"runtime_done\"");

    let (trace_id, span_id) = assert_belong_to_one_span(&logs[0..2]);

    // A failed init span is delivered
    let spans = extract_spans(&coralogix).await;
    assert_eq!(spans.len(), 1);
    assert_eq!(&spans[0].name, "my-lambda init");
    assert_eq!(
        spans[0].status.as_ref().unwrap().code,
        StatusCode::Error as i32
    );
    assert_eq!(&spans[0].trace_id, &trace_id);
    assert_eq!(&spans[0].span_id, &span_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handling_timeout_in_low_overhead_and_otel_mode() {
    // initialize_logging();
    let coralogix = Arc::new(CoralogixClientMock::default());
    let ts = make_telemetry_service_with_config(
        function_context_provider(),
        Some(coralogix.clone()),
        telemetry_service_config(LowOverhead, OtelInstrumentation),
    );

    let ready_to_freez = handle_invoke_event(&ts, REQUEST1_ID, TRACING1);
    give_events_time_to_process().await;

    expect_readiness(ready_to_freez).await;

    ts.handle_incoming_telemetry(vec![
        init_start(),
        init_runtime_done(),
        init_report(),
        platform_start(REQUEST1_ID, &TRACE_CONTEXT1),
        function_log("function log"),
    ])
    .await
    .unwrap();

    ts.handle_otlp_spans(single_span_export("function span"))
        .await
        .unwrap();

    let ready_to_shutdown = handle_shutdown_event(&ts);
    give_events_time_to_process().await;

    ts.handle_incoming_telemetry(vec![platform_runtime_done(REQUEST1_ID, &TRACE_CONTEXT1)])
        .await
        .unwrap();

    expect_readiness(ready_to_shutdown).await;

    let logs = extract_log_records(&coralogix).await;
    let spans = extract_spans(&coralogix).await;

    assert_eq!(logs.len(), 3);
    assert_log_contains(&logs[0], "\"start\"");
    assert_log_contains(&logs[1], "\"function log\"");
    assert_log_contains(&logs[2], "\"runtime_done\"");

    assert_eq!(spans.len(), 2); // that one function span that was delivered to telemetry-exporter early is delivered to coralogix
    // we don't expect the main function span to be delivered when a timeout occurs.
    // also the init span is delivered
    assert_eq!(&spans[0].name, "function span");
    assert_eq!(&spans[1].name, "my-lambda init");

    // The logs correlate with each other and belong to the trace, but since we never received a main function span, there is no span to corelate with
    let (trace_id, _span_id) = assert_belong_to_one_span(&logs[0..3]);
    assert_eq!(&spans[1].trace_id, &trace_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handling_timeout_in_report_after_invocation_and_otel_mode() {
    // initialize_logging();
    let coralogix = Arc::new(CoralogixClientMock::default());
    let ts = make_telemetry_service_with_config(
        function_context_provider(),
        Some(coralogix.clone()),
        telemetry_service_config(ReportAfterInvocation, OtelInstrumentation),
    );

    let ready_to_freez = handle_invoke_event(&ts, REQUEST1_ID, TRACING1);
    give_events_time_to_process().await;

    ts.handle_incoming_telemetry(vec![
        init_start(),
        init_runtime_done(),
        init_report(),
        platform_start(REQUEST1_ID, &TRACE_CONTEXT1),
        function_log("function log"),
    ])
    .await
    .unwrap();

    ts.handle_otlp_spans(single_span_export("function span"))
        .await
        .unwrap();

    // ReportAfterInvocation will wait for runtime done before it starts listening for shutdown
    ts.handle_incoming_telemetry(vec![platform_runtime_done(REQUEST1_ID, &TRACE_CONTEXT1)])
        .await
        .unwrap();

    expect_readiness(ready_to_freez).await;

    let ready_to_shutdown = handle_shutdown_event(&ts);
    give_events_time_to_process().await;

    expect_readiness(ready_to_shutdown).await;

    let logs = extract_log_records(&coralogix).await;
    let spans = extract_spans(&coralogix).await;

    assert_eq!(logs.len(), 3);
    assert_log_contains(&logs[0], "\"start\"");
    assert_log_contains(&logs[1], "\"function log\"");
    assert_log_contains(&logs[2], "\"runtime_done\"");

    assert_eq!(spans.len(), 2); // that one function span that was delivered to telemetry-exporter early is delivered to coralogix
    // we don't expect the main function span to be delivered when a timeout occurs.
    // also the init span is delivered
    assert_eq!(&spans[0].name, "function span");
    assert_eq!(&spans[1].name, "my-lambda init");

    // The logs correlate with each other and belong to the trace, but since we never received a main function span, there is no span to corelate with
    let (trace_id, _span_id) = assert_belong_to_one_span(&logs[0..3]);
    assert_eq!(&spans[1].trace_id, &trace_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handling_logs_after_runtime_done_in_otel_mode() {
    // initialize_logging();
    let coralogix = Arc::new(CoralogixClientMock::default());
    let ts = make_telemetry_service_otel(function_context_provider(), Some(coralogix.clone()));

    // Invocation 1 //

    let ready_to_freez = handle_invoke_event(&ts, REQUEST1_ID, TRACING1);
    give_events_time_to_process().await;

    ts.handle_incoming_telemetry(vec![
        init_start(),
        init_runtime_done(),
        init_report(),
        platform_start(REQUEST1_ID, &TRACE_CONTEXT1),
        function_log("function log 1"),
    ])
    .await
    .unwrap();

    ts.handle_otlp_spans(main_span_export()).await.unwrap();

    ts.handle_incoming_telemetry(vec![platform_runtime_done(REQUEST1_ID, &TRACE_CONTEXT1)])
        .await
        .unwrap();

    expect_readiness(ready_to_freez).await;

    // Invocation 2 //

    let ready_to_freez = handle_invoke_event(&ts, REQUEST2_ID, TRACING2);
    give_events_time_to_process().await;

    ts.handle_otlp_spans(main_span_export()).await.unwrap();

    ts.handle_incoming_telemetry(vec![
        // a background thread kept producing logs after the handler was done
        // because OTEL mode buffers logs until runtime_done, and this log arrives after runtime_done, there's a risk this log could be lost
        function_log("function log 2"),
        platform_report(REQUEST1_ID, &TRACE_CONTEXT1), // report for previous invocation
        platform_start(REQUEST2_ID, &TRACE_CONTEXT2),
        function_log("function log 3"),
        platform_runtime_done(REQUEST2_ID, &TRACE_CONTEXT2),
    ])
    .await
    .unwrap();

    expect_readiness(ready_to_freez).await;

    let logs = extract_log_records(&coralogix).await;
    let spans = extract_spans(&coralogix).await;

    assert_eq!(logs.len(), 8);
    assert_log_contains(&logs[0], "\"start\"");
    assert_log_contains(&logs[1], "\"function log 1\"");
    assert_log_contains(&logs[2], "\"runtime_done\"");
    assert_log_contains(&logs[3], "\"function log 2\"");
    assert_log_contains(&logs[4], "\"report\"");

    assert_log_contains(&logs[5], "\"start\"");
    assert_log_contains(&logs[6], "\"function log 3\"");
    assert_log_contains(&logs[7], "\"runtime_done\"");

    assert_eq!(spans.len(), 3);
    assert_eq!(&spans[0].name, "main function span");
    assert_eq!(&spans[1].name, "my-lambda init");
    assert_eq!(&spans[2].name, "main function span");

    let (trace_id, span_id) = assert_belong_to_one_span(&logs[0..5]);
    assert_eq!(&spans[0].trace_id, &trace_id);
    assert_eq!(&spans[0].span_id, &span_id);

    assert_eq!(&spans[0].parent_span_id, &spans[1].parent_span_id);

    let (trace_id, span_id) = assert_belong_to_one_span(&logs[5..8]);
    assert_eq!(&spans[2].trace_id, &trace_id);
    assert_eq!(&spans[2].span_id, &span_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn low_overhead_sends_only_at_the_beginning_of_an_invocation_once_delay_elapses() {
    // initialize_logging();
    let coralogix = Arc::new(CoralogixClientMock::default());
    let ts = make_telemetry_service_with_config(
        function_context_provider(),
        Some(coralogix.clone()),
        telemetry_service_config(LowOverhead, OtelInstrumentation),
    );

    // Invocation 1 //

    let ready_to_freez = handle_invoke_event(&ts, REQUEST1_ID, TRACING1);
    give_events_time_to_process().await;
    expect_readiness(ready_to_freez).await;

    ts.handle_otlp_spans(main_span_export()).await.unwrap();

    ts.handle_incoming_telemetry(vec![
        init_start(),
        init_runtime_done(),
        init_report(),
        platform_start(REQUEST1_ID, &TRACE_CONTEXT1),
        function_log("function log 1"),
        platform_runtime_done(REQUEST1_ID, &TRACE_CONTEXT1),
    ])
    .await
    .unwrap();

    expect_no_telemetry(&coralogix).await;

    // Invocation 2 //

    let ready_to_freez = handle_invoke_event(&ts, REQUEST2_ID, TRACING2);
    give_events_time_to_process().await;
    expect_readiness(ready_to_freez).await;

    ts.handle_otlp_spans(main_span_export()).await.unwrap();

    ts.handle_incoming_telemetry(vec![
        platform_report(REQUEST1_ID, &TRACE_CONTEXT1),
        platform_start(REQUEST2_ID, &TRACE_CONTEXT2),
        function_log("function log 2"),
        platform_runtime_done(REQUEST2_ID, &TRACE_CONTEXT2),
    ])
    .await
    .unwrap();

    expect_no_telemetry(&coralogix).await;

    // enough time elapses to match reporting delay
    sleep_ms(350).await;

    // Invocation 3 //

    let ready_to_freez = handle_invoke_event(&ts, REQUEST3_ID, TRACING3);
    give_events_time_to_process().await;
    expect_readiness(ready_to_freez).await;

    // Telemetry from invocations 1 and 2 is sent at the beginning of invocation 3
    let logs = extract_log_records(&coralogix).await;
    assert_eq!(logs.len(), 7);
    let spans = extract_spans(&coralogix).await;
    assert_eq!(spans.len(), 3);

    assert_eq!(logs.len(), 7);
    assert_log_contains(&logs[0], "\"start\"");
    assert_log_contains(&logs[1], "\"function log 1\"");
    assert_log_contains(&logs[2], "\"runtime_done\"");
    assert_log_contains(&logs[3], "\"report\"");

    assert_log_contains(&logs[4], "\"start\"");
    assert_log_contains(&logs[5], "\"function log 2\"");
    assert_log_contains(&logs[6], "\"runtime_done\"");

    assert_eq!(spans.len(), 3);
    assert_eq!(&spans[0].name, "main function span");
    assert_eq!(&spans[1].name, "main function span");
    assert_eq!(&spans[2].name, "my-lambda init");

    let (trace_id, span_id) = assert_belong_to_one_span(&logs[0..4]);
    assert_eq!(&spans[0].trace_id, &trace_id);
    assert_eq!(&spans[0].span_id, &span_id);

    assert_eq!(&spans[0].parent_span_id, &spans[2].parent_span_id);

    let (trace_id, span_id) = assert_belong_to_one_span(&logs[4..7]);
    assert_eq!(&spans[1].trace_id, &trace_id);
    assert_eq!(&spans[1].span_id, &span_id);
}

// TODO In OTEL mode this works only for function spans / metrics, because logs are buffered until runtime_done.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn report_during_and_after_invocation_sends_once_delay_elapses() {
    // initialize_logging();
    let coralogix = Arc::new(CoralogixClientMock::default());
    let ts = make_telemetry_service_with_config(
        function_context_provider(),
        Some(coralogix.clone()),
        telemetry_service_config(ReportDuringAndAfterInvocation, OtelInstrumentation),
    );

    let ready_to_freez = handle_invoke_event(&ts, REQUEST1_ID, TRACING1);
    give_events_time_to_process().await;

    ts.handle_incoming_telemetry(vec![
        init_start(),
        init_runtime_done(),
        init_report(),
        platform_start(REQUEST1_ID, &TRACE_CONTEXT1),
        function_log("function log 1"),
    ])
    .await
    .unwrap();

    ts.handle_otlp_spans(single_span_export("function span 1"))
        .await
        .unwrap();

    sleep_ms(350).await;

    let logs = extract_log_records(&coralogix).await;
    let spans = extract_spans(&coralogix).await;

    assert_eq!(logs.len(), 0);
    assert_eq!(spans.len(), 1);
    assert_eq!(&spans[0].name, "function span 1");

    ts.handle_otlp_spans(main_span_export()).await.unwrap();

    ts.handle_incoming_telemetry(vec![platform_runtime_done(REQUEST1_ID, &TRACE_CONTEXT1)])
        .await
        .unwrap();

    expect_readiness(ready_to_freez).await;

    let logs = extract_log_records(&coralogix).await;
    let spans = extract_spans(&coralogix).await;

    assert_eq!(logs.len(), 3);
    assert_log_contains(&logs[0], "\"start\"");
    assert_log_contains(&logs[1], "\"function log 1\"");
    assert_log_contains(&logs[2], "\"runtime_done\"");

    assert_eq!(spans.len(), 3);
    assert_eq!(&spans[1].name, "main function span");
    assert_eq!(&spans[2].name, "my-lambda init");

    let (trace_id, span_id) = assert_belong_to_one_span(&logs[0..3]);
    assert_eq!(&spans[1].trace_id, &trace_id);
    assert_eq!(&spans[1].span_id, &span_id);

    assert_eq!(&spans[0].parent_span_id, &spans[1].parent_span_id);
}

// TODO test a scenario where we receive a log belonging to a function after the platform.report event (yes, this happened)
