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

use super::function_context::FunctionContextProvider;
use super::otel_metrics::OtelMetricsAccumulator;
use super::otlp_wrappers::*;
use super::processor::telemetry_processor;
use super::sending_supervisor::SendingSupervisor;
use super::telemetry_sender::DynBatchTelemetrySender;
use super::*;
use crate::Error;
use crate::config::app_config::{
    OtelMetricsMode, PlatformMetricsMode, TelemetryReportingStrategy, TelemetryServiceConfig,
};
use crate::proto::opentelemetry::proto::collector::metrics::v1::ExportMetricsServiceRequest;
use crate::proto::opentelemetry::proto::collector::trace::v1::ExportTraceServiceRequest;
use crate::proto::opentelemetry::proto::logs::v1::{LogRecord, ResourceLogs};
use crate::proto::opentelemetry::proto::metrics::v1::{
    self as otel_metrics, ResourceMetrics, ScopeMetrics,
};
use crate::proto::opentelemetry::proto::trace::v1::{
    self as otel_trace, ResourceSpans, ScopeSpans,
};
use crate::telemetry::telemetry_sender::TelemetryBatch;
use lambda_extension::{InvokeEvent, LambdaTelemetry, LambdaTelemetryRecord, ShutdownEvent};
use processor::PlatformMetricsState;
use std::cmp::max;
use std::collections::VecDeque;
use std::mem::take;
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::select;
use tokio::sync::{Mutex, mpsc};
use tokio::time::timeout;
use tracing::{debug, error, info, trace, warn};

pub struct TelemetryService {
    config: Arc<TelemetryServiceConfig>,
    telemetry_sender: DynBatchTelemetrySender,
    function_context_provider: FunctionContextProvider,
    signal_sender: mpsc::UnboundedSender<Signal>,
    signal_receiver: Mutex<mpsc::UnboundedReceiver<Signal>>,
    state: Arc<Mutex<TelemetryServiceState>>,
}

struct TelemetryServiceState {
    // inputs
    // The service receives invoke events and telemetry events independently and needs to merge information from them.
    // Typically it will receive invoke event first and then the corresponding telemetry.
    // But it may also receive multiple invoke events before receiving telemetry corresponding to them all if wait_after_invocation is set to false.
    // Or in rare circumstances it may receive telemetry before the invoke event. (Never observed in practice, but there's nothing guaranteeing that it cannot happen, so it's better to be prepared)
    // These queues help to buffer events on one side until the other side caches up and they can be processed.
    invoke_event_queue: VecDeque<InvokeEventDataWithSpans>,
    telemetry_event_queue: VecDeque<LambdaTelemetry>,
    function_spans_buffer: Vec<ResourceSpans>,

    // state of processing
    half_processed_event_buffer: VecDeque<LambdaTelemetry>,
    init_handling_started: bool,
    init_handled: bool,
    shutdown_flush_started: bool,
    invocation_state: Option<InvocationProcessingState>,
    metrics_state: PlatformMetricsState,
    metrics_accumulator: OtelMetricsAccumulator,

    // outputs
    output_buffers: OutputBuffers,

    // coordination
    should_send_by: Option<Instant>,
    should_send: bool,
    received_first_invocation: bool,
    should_not_receive_otel_now: bool,
}

#[derive(Debug, Default)]
pub struct OutputBuffers {
    pub logs_buffer: Vec<LogRecord>,
    pub spans_buffer: Vec<otel_trace::Span>,
    pub function_metrics_buffer: Vec<otel_metrics::ResourceMetrics>,
    function_spans_buffer: Vec<otel_trace::ScopeSpans>,
    function_spans_count: usize,
    pub epsagon_traces_buffer: Vec<String>,
}

impl OutputBuffers {
    pub fn add_function_resource_spans(&mut self, rss: Vec<otel_trace::ResourceSpans>) {
        for rs in rss.into_iter() {
            self.add_function_scope_spans(rs.scope_spans);
        }
    }

    pub fn add_function_scope_spans(&mut self, ss: Vec<otel_trace::ScopeSpans>) {
        for ss in ss.into_iter() {
            self.function_spans_count += ss.spans.len();
            self.function_spans_buffer.push(ss);
        }
    }

    pub fn take_function_spans(&mut self) -> Vec<otel_trace::ScopeSpans> {
        self.function_spans_count = 0;
        take(&mut self.function_spans_buffer)
    }
}

enum Signal {
    ShouldSend,
    InvocationDone, // TODO consider adding request id. This is necessary if we want to use these signals in LowOverhead mode.
}

impl TelemetryService {
    pub fn new(
        config: Arc<TelemetryServiceConfig>,
        function_context_provider: FunctionContextProvider,
        telemetry_sender: DynBatchTelemetrySender,
    ) -> TelemetryService {
        let (signal_sender, signal_receiver) = mpsc::unbounded_channel::<Signal>();
        let metrics_state =
            PlatformMetricsState::for_mode(config.processor_config.platform_metrics_mode);
        TelemetryService {
            config,
            function_context_provider,
            telemetry_sender,
            signal_sender,
            signal_receiver: Mutex::new(signal_receiver),
            state: Arc::new(Mutex::new(TelemetryServiceState {
                invoke_event_queue: VecDeque::new(),
                telemetry_event_queue: VecDeque::new(),
                function_spans_buffer: Vec::new(),
                half_processed_event_buffer: VecDeque::new(),
                init_handling_started: false,
                init_handled: false,
                shutdown_flush_started: false,
                invocation_state: None,
                metrics_state,
                metrics_accumulator: OtelMetricsAccumulator::default(),
                output_buffers: OutputBuffers::default(),
                should_send_by: None,
                should_send: false,
                received_first_invocation: false,
                should_not_receive_otel_now: false,
            })),
        }
    }

    pub async fn handle_invoke_event(&self, e: InvokeEvent) -> Result<(), Error> {
        debug!("Handling invoke event {:?}", e);

        let function_context = self.function_context_provider.on_invoke_event(&e).await?;

        if self.config.reporting_strategy == TelemetryReportingStrategy::LowOverhead {
            // Drain signals to avoid leaking memory
            // This has to happen before pushing the invoke event or else the InvocationDone for current invocation could be drained. (It's unused in LowOverhead for now, but could be in the future)
            while self.signal_receiver.lock().await.try_recv().is_ok() {}
        }

        {
            let mut state = self.state.lock().await;
            state.received_first_invocation = true;
            state
                .invoke_event_queue
                .push_back(InvokeEventDataWithSpans {
                    deadline_ms: e.deadline_ms,
                    request_id: e.request_id,
                    invoked_function_arn: e.invoked_function_arn,
                    tracing: InvokeTraceContext {
                        r#type: e.tracing.r#type,
                        value: e.tracing.value,
                    },
                    spans: Vec::new(),
                    function_context: function_context.clone(),
                });

            // Just pushed a new invoke event, and there may already be some telemetry waiting for it.
            self.handle_telemetry(&mut state);
        }

        let mut sending_supervisor = SendingSupervisor::new(self.telemetry_sender.clone());

        if self.config.reporting_strategy == TelemetryReportingStrategy::LowOverhead {
            // sending the telemetry when an invocation begins means that the operation happens in parallel to the function execution, reducing overhead.
            {
                let mut state = self.state.lock().await;
                if state.should_send || Self::should_send_by_now(&state) {
                    self.send_telemetry(&mut state, &function_context, &mut sending_supervisor);
                }
            }
        } else {
            loop {
                {
                    let mut state = self.state.lock().await;
                    if state.should_send || Self::should_send_by_now(&state) {
                        self.send_telemetry(&mut state, &function_context, &mut sending_supervisor);
                    }
                }
                // sometimes this will wake us up more times than necessary (for example there may be 2 ShouldSend enqueued, but that's not a problem because we check the `should_send` flag in state )
                match self.signal_receiver.lock().await.recv().await {
                    Some(Signal::ShouldSend) => (),
                    Some(Signal::InvocationDone) => break, // With the current implementation there's always ShouldSend before an InvocationDone with the relevant strategies.
                    None => panic!("Signal channel is closed!"),
                }
            }
        }

        if self.config.reporting_strategy != TelemetryReportingStrategy::LowOverhead {
            let mut state = self.state.lock().await;
            state.should_not_receive_otel_now = true;
        }

        // The timeout is here in order to guarantee that we will eventually proceed and will not block the lambda instance
        let _ = timeout(
            Duration::from_secs(5), // TODO consider reducing this timeout for lambdas with a short timeout
            sending_supervisor.await_in_flight_sends(),
        )
        .await;

        debug!("Signalling readiness to receive next invocation or be frozen.");

        if self.config.reporting_strategy != TelemetryReportingStrategy::LowOverhead {
            let mut state = self.state.lock().await;
            state.should_not_receive_otel_now = false;
        }

        Ok(())
    }

    pub async fn handle_shutdown_event(&self, e: ShutdownEvent) -> Result<(), Error> {
        let received_first_invocation = {
            let state = self.state.lock().await;
            state.received_first_invocation
        };

        let remaining = Self::remaining_time_ms_str(&e);
        info!(
            "Lambda instance is shutting down. {}ms/2000ms remaining. {:?}",
            remaining, e
        );

        // In REPORT_DURING_AND_AFTER_INVOCATION and REPORT_AFTER_INVOCATION strategies we wait for Telemetry API to send the last batch of telemetry containing runtime_done, before we call next and receive shutdown.
        // In that case nothing special needs to happen on shutdown.
        // But there are situations where shutdown happens and there is still some telemetry to send.
        // One such situation is when using LOW_OVERHEAD
        // Another is when the last batch didn't contain a runtimeDone (this can happen for example when the function crashes during init phase)
        // We want to give the telemetry api a change to deliver us a runtimeDone and process if nicely, and only after that happens, or max_shutdown_flush_delay elapses, we proceed to processing remaining telemetry in degraded mode.
        // If a crash occurred before first invoke event, then the function_context is None, and we need to fall back to a dummy one (process in degraded mode).
        if self.config.reporting_strategy == TelemetryReportingStrategy::LowOverhead
            || !received_first_invocation
        {
            // TODO stop sleeping when invocation_done for the MATCHING request_id is received.
            debug!("Waiting for more telemetry to arrive before flushing telemetry.");
            tokio::time::sleep(self.config.max_shutdown_flush_delay).await;
        }

        let function_context = self
            .function_context_provider
            .get_function_context_even_if_degraded()
            .await;

        let mut sending_supervisor = SendingSupervisor::new(self.telemetry_sender.clone());
        {
            let mut state = self.state.lock().await;
            self.flush_enqueued_telemetry(&mut state, function_context.clone());
            self.send_telemetry(&mut state, &function_context, &mut sending_supervisor);
        }

        let sleep_for = Self::remaining_time_ms(&e).map_or(0, |remaining| max(remaining - 50, 0));
        select! {
            _ = tokio::time::sleep(Duration::from_millis(sleep_for)) => {
                // We want to log this message just before (50ms) AWS shuts down the instance.
                warn!("Failed to send all telemetry on shutdown.")
            }
            _ = sending_supervisor.await_in_flight_sends() => {
                info!("Finished sending telemetry on shutdown")
            }
        }

        // AWS doesn't really care if and when we complete. It shuts down in 2 seconds and that's it.
        Ok(())
    }

    fn remaining_time_ms_str(e: &ShutdownEvent) -> String {
        Self::remaining_time_ms(e).map_or_else(|| "?".to_owned(), |ms| ms.to_string())
    }

    fn remaining_time_ms(e: &ShutdownEvent) -> Option<u64> {
        let deadline =
            OffsetDateTime::from_unix_timestamp_nanos(e.deadline_ms as i128 * 1_000_000).ok()?;
        let remaining_time = deadline - OffsetDateTime::now_utc();
        u64::try_from(remaining_time.whole_milliseconds()).ok()
    }

    fn flush_enqueued_telemetry(
        &self,
        state: &mut TelemetryServiceState,
        function_context: Arc<FunctionContext>,
    ) {
        info!("Flushing enqueued telemetry");
        state.shutdown_flush_started = true;
        let events = take(&mut state.telemetry_event_queue);
        info!("Flushing {} enqueued telemetry events", events.len());
        let processing_result = self.process_in_degraded_mode(state, function_context, events);
        match processing_result {
            Ok(_) => (),
            Err(error) => error!(?error, "Failed to process telemetry in degraded mode."),
        }
    }

    pub async fn handle_otlp_spans(&self, e: ExportTraceServiceRequest) -> Result<(), Error> {
        {
            trace!("Handling function spans: {:?}", &e.resource_spans);
            let mut state_guard = self.state.lock().await;
            let state = state_guard.deref_mut();

            if state.should_not_receive_otel_now {
                debug!("Should not receive OTLP spans now.");
            }

            match state.invoke_event_queue.back_mut() {
                Some(last_event) => {
                    last_event.spans.extend(e.resource_spans);
                }
                None => match state.invocation_state.as_mut() {
                    Some(invocation_state) if invocation_state.runtime_done_data.is_some() => {
                        // Instrumentation shouldn't send spans after runtime is done, so these spans most likely come from a new invocation that already started,
                        // but we haven't yet received the invocation event or the report event for previous invocation.
                        state.function_spans_buffer.extend(e.resource_spans);
                    }
                    Some(invocation_state) => {
                        telemetry_processor::process_function_spans(
                            invocation_state,
                            e.resource_spans,
                            &self.config.processor_config,
                            &mut state.output_buffers,
                        )?;
                        let function_spans_count = state.output_buffers.function_spans_count;
                        trace!("Number of spans in buffer: {}", function_spans_count);

                        // This logic protects the telemetry exporter from accumulating too many spans in memory
                        // It's important that the function itself doesn't wait for the transmission to coralogix to complete,
                        // as otherwise it would back-pressure the instrumentation's OTEL exporter, which could lead to loosing spans.
                        if function_spans_count >= self.config.span_sending_threshold {
                            debug!(
                                "Will send telemetry early to coralogix, because there are already {function_spans_count} spans awaiting.",
                            );
                            Self::signal_should_send(&self.signal_sender, state);
                        } else if self.config.reporting_strategy.send_after_delay() {
                            self.signal_should_send_after_delay(state);
                        }
                    }
                    None => {
                        state.function_spans_buffer.extend(e.resource_spans);
                    }
                },
            };
        }
        Ok(())
    }

    pub async fn handle_otlp_metrics(&self, e: ExportMetricsServiceRequest) -> Result<(), Error> {
        trace!("Handling function metrics: {:?}", &e.resource_metrics);
        let mut state_guard = self.state.lock().await;
        let state = state_guard.deref_mut();

        if state.should_not_receive_otel_now {
            debug!("Should not receive OTLP metric now.");
        }

        if self.config.reporting_strategy.send_after_delay() {
            self.signal_should_send_after_delay(state);
        }
        telemetry_processor::process_function_metrics(
            e.resource_metrics,
            &self.config.processor_config,
            &mut state.metrics_accumulator,
            &mut state.output_buffers,
        );
        Ok(())
    }

    pub async fn handle_incoming_telemetry(
        &self,
        telemetry_events: Vec<LambdaTelemetry>,
    ) -> Result<(), Error> {
        let number_of_events = telemetry_events.len();
        if tracing::enabled!(tracing::Level::TRACE) {
            trace!(
                "Handling {} telemetry events: {:?} ",
                number_of_events, telemetry_events
            );
        } else {
            debug!("Handling {} telemetry events", number_of_events);
        }

        let mut state = self.state.lock().await;
        if self.config.reporting_strategy.send_after_delay()
            && let Some(oldest_event) = telemetry_events.first()
        {
            // we can assume the telemetry is sorted by time
            self.signal_should_send_after_delay_from_event(&mut state, oldest_event);
        }
        state.telemetry_event_queue.extend(telemetry_events);
        self.handle_telemetry(&mut state);
        trace!("Done handling {} telemetry events", number_of_events);
        Ok(())
    }

    fn handle_telemetry(&self, state: &mut TelemetryServiceState) {
        match self.try_processing_telemetry(state) {
            Ok(invocation_done) => {
                if invocation_done {
                    self.signal_invocation_done();
                }
            }
            Err(error) => {
                // Giving up and letting the lambda proceed
                // TODO Maybe the state should be cleared to some extent here?
                error!(?error, "Failed to handle telemetry");
                self.signal_invocation_done();
            }
        }
    }

    fn try_processing_telemetry(&self, state: &mut TelemetryServiceState) -> Result<bool, Error> {
        if state.shutdown_flush_started {
            // Typically remaining telemetry should be delivered right before shutdown starts, sometimes right after it starts,
            // so the case when telemetry arrives after shutdown flush should be very rare.
            warn!(
                "Received telemetry while shutting down, after flushing. That telemetry will be lost."
            );
            Ok(false)
        } else {
            let invocation_done = self.process_telemetry_events(state)?;

            if self.config.reporting_strategy.send_after_invocation() && invocation_done {
                debug!("Will send telemetry to coralogix because invocation is done.");
                Self::signal_should_send(&self.signal_sender, state);
            }
            Ok(invocation_done)
        }
    }

    fn signal_invocation_done(&self) {
        match self.signal_sender.send(Signal::InvocationDone) {
            Ok(_) => (),
            Err(error) => error!(?error, "signal_readiness failed: Err"),
        }
    }

    fn signal_should_send_after_delay_from_event(
        &self,
        state: &mut TelemetryServiceState,
        event: &LambdaTelemetry,
    ) {
        if let Ok(time) = self.time_for_sending_event(event) {
            self.signal_should_send_by(state, time);
        }
    }

    fn time_for_sending_event(&self, event: &LambdaTelemetry) -> Result<Instant, Error> {
        let age = OffsetDateTime::now_utc() - event_timestamp(event)?;
        let time = Instant::now() + self.config.reporting_delay - age;
        Ok(time)
    }

    // For traces and metrics we refer to the time when they were received, not the timestamps.
    // This is because we have no influence on how long they are buffered by OTEL SDK, and if they are buffered for long, then they would keep triggering send operations.
    fn signal_should_send_after_delay(&self, state: &mut TelemetryServiceState) {
        self.signal_should_send_by(state, Instant::now() + self.config.reporting_delay)
    }

    fn signal_should_send_by(&self, state: &mut TelemetryServiceState, time: Instant) {
        if state.should_send_by.is_none_or(|x| time <= x) {
            state.should_send_by = Some(time);
            let time_to_sleep = time - Instant::now(); // this subtraction saturates at zero, so if `now`` is after `by`` then we get zero
            if time_to_sleep.is_zero() {
                trace!("Should send on the next occasion because reporting_delay elapsed");
                state.should_send_by = None;
                Self::signal_should_send(&self.signal_sender, state);
            } else {
                let state_mutex = self.state.clone();
                let signal_sender = self.signal_sender.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(time_to_sleep).await;
                    let mut state = state_mutex.lock().await;
                    if Self::should_send_by_now(&state) {
                        trace!("Should send on the next occasion because reporting_delay elapsed");
                        state.should_send_by = None;
                        Self::signal_should_send(&signal_sender, &mut state);
                    }
                });
            }
        }
    }

    fn should_send_by_now(state: &TelemetryServiceState) -> bool {
        let now = Instant::now();
        state.should_send_by.is_some_and(|by| now >= by)
    }

    fn signal_should_send(
        signal_sender: &mpsc::UnboundedSender<Signal>,
        state: &mut TelemetryServiceState,
    ) {
        state.should_send = true;
        match signal_sender.send(Signal::ShouldSend) {
            Ok(_) => (),
            Err(error) => error!(?error, "Failed to send signal"),
        };
    }

    /// Returns true if at least one PlatformRuntimeDone event was processed
    fn process_telemetry_events(
        &self,
        service_state: &mut TelemetryServiceState,
    ) -> Result<bool, Error> {
        let mut invocation_done = false;
        loop {
            let state: &mut InvocationProcessingState =
                match service_state.invocation_state.as_mut() {
                    Some(invocation_state) =>
                    // Continue processing telemetry corresponding to the invocation
                    {
                        invocation_state
                    }
                    None => {
                        match service_state.invoke_event_queue.pop_front() {
                            Some(mut invoke_event) => {
                                debug!(
                                    "Processing invoke event for {}",
                                    invoke_event.invoked_function_arn
                                );

                                // It's time to start processing telemetry from a new invocation
                                let mut new_invocation_state = InvocationProcessingState::new(
                                    invoke_event.function_context.clone(),
                                    InvocationContext::try_from(&invoke_event)?,
                                    self.config.processor_config.trace_sampling_mode,
                                    self.config.processor_config.tracing_mode,
                                );

                                telemetry_processor::process_function_spans(
                                    &mut new_invocation_state,
                                    take(&mut service_state.function_spans_buffer),
                                    &self.config.processor_config,
                                    &mut service_state.output_buffers,
                                )?;

                                telemetry_processor::process_function_spans(
                                    &mut new_invocation_state,
                                    take(&mut invoke_event.spans),
                                    &self.config.processor_config,
                                    &mut service_state.output_buffers,
                                )?;

                                service_state.invocation_state = Some(new_invocation_state);
                                service_state.invocation_state.as_mut().unwrap()
                            }
                            None => break, // There's no invocation to process. Even if there is some telemetry, there is not enough context to process it yet. This case should be rare, but there's no guarantee that it won't happen.
                        }
                    }
                };

            let function_context = state.function_context.clone();

            let event = match service_state.telemetry_event_queue.pop_front() {
                Some(e) => e,
                None => break,
            };

            if !service_state.init_handling_started {
                match &event.record {
                    LambdaTelemetryRecord::PlatformInitStart { .. } => {
                        if !service_state.half_processed_event_buffer.is_empty() {
                            let events = take(&mut service_state.half_processed_event_buffer);
                            debug!(
                                "Processing {} events in degraded mode. These events most likely come from a previous instance of the lambda function.",
                                events.len()
                            );

                            self.process_in_degraded_mode(service_state, function_context, events)?;
                        }

                        service_state.init_handling_started = true;
                        service_state.half_processed_event_buffer.push_back(event)
                    }
                    _ => service_state.half_processed_event_buffer.push_back(event),
                }
            } else if !service_state.init_handled {
                match &event.record {
                    LambdaTelemetryRecord::PlatformInitStart { .. } => {
                        // That's a second PlatformInitStart. Processing all data related to the previous one in degraded mode and starting over.
                        let events = take(&mut service_state.half_processed_event_buffer);
                        debug!(
                            "Processing {} events in degraded mode. These events most likely come from a previous instance of the lambda function.",
                            events.len()
                        );
                        self.process_in_degraded_mode(service_state, function_context, events)?;

                        service_state.half_processed_event_buffer.push_back(event);
                    }
                    LambdaTelemetryRecord::PlatformStart { request_id, .. } => {
                        if request_id == &state.invocation_context.request_id {
                            // change state and return the event back to the front of the queue to be processed in next loop iteration
                            debug!("Processing PlatformStart for the expected {}", request_id);
                            service_state.init_handled = true;
                            service_state.telemetry_event_queue.push_front(event);

                            let events = take(&mut service_state.half_processed_event_buffer);
                            debug!("Processing {} buffered init events", events.len());
                            for e in events.into_iter() {
                                telemetry_processor::process_telemetry(
                                    &self.config.processor_config,
                                    e,
                                    state,
                                    &mut service_state.metrics_state,
                                    &mut service_state.output_buffers,
                                )?;
                            }
                        } else {
                            warn!(
                                "Processing PlatformStart for {} when expecting PlatformStart for {}",
                                request_id, state.invocation_context.request_id
                            );
                            service_state.half_processed_event_buffer.push_back(event);
                        }
                    }
                    _ => service_state.half_processed_event_buffer.push_back(event),
                }
            } else {
                if let LambdaTelemetryRecord::PlatformRuntimeDone { .. } = &event.record {
                    debug!("Processing RuntimeDone event.");
                    invocation_done = true;
                };

                telemetry_processor::process_telemetry(
                    &self.config.processor_config,
                    event,
                    state,
                    &mut service_state.metrics_state,
                    &mut service_state.output_buffers,
                )?;

                if state.report_data.is_some() {
                    service_state.invocation_state = None;
                }
            }
        }
        Ok(invocation_done)
    }

    fn process_in_degraded_mode(
        &self,
        state: &mut TelemetryServiceState,
        function_context: Arc<FunctionContext>,
        events: VecDeque<LambdaTelemetry>,
    ) -> Result<(), Error> {
        let mut degraded_processing_state = DegradedProcessingState::new(function_context);
        for event in events.into_iter() {
            telemetry_processor::process_telemetry_in_degraded_mode(
                &self.config.processor_config,
                event,
                &mut degraded_processing_state,
                &mut state.metrics_state,
                &mut state.output_buffers,
            )?;
        }
        Ok(())
    }

    // All send operations should happen while handling invoke or shutdown event. This way the environment won't get frozen while there are network requests in flight.
    fn send_telemetry(
        &self,
        state: &mut TelemetryServiceState,
        function_context: &FunctionContext,
        sending_supervisor: &mut SendingSupervisor,
    ) {
        state.should_send = false;
        state.should_send_by = None;
        let batch = self.prepare_telemetry_batch(state, function_context);
        sending_supervisor.send(batch);
    }

    fn prepare_telemetry_batch(
        &self,
        state: &mut TelemetryServiceState,
        function_context: &FunctionContext,
    ) -> TelemetryBatch {
        TelemetryBatch {
            logs: self.prepare_logs(state, function_context),
            spans: self.prepare_spans(state, function_context),
            metrics: self.prepare_metrics(state, function_context),
            epsagon_traces: self.prepare_epsagon_traces(state),
        }
    }

    fn prepare_logs(
        &self,
        state: &mut TelemetryServiceState,
        function_context: &FunctionContext,
    ) -> Vec<ResourceLogs> {
        vec![wrap_logs_in_resource(
            vec![wrap_logs_in_scope(
                take(&mut state.output_buffers.logs_buffer),
                function_context,
            )],
            function_context,
            &self.config.processor_config.resource_attributes.logs,
        )]
    }

    fn prepare_spans(
        &self,
        state: &mut TelemetryServiceState,
        function_context: &FunctionContext,
    ) -> Vec<ResourceSpans> {
        let mut scope_spans: Vec<ScopeSpans> = state.output_buffers.take_function_spans();
        let plartform_spans = take(&mut state.output_buffers.spans_buffer);
        if !plartform_spans.is_empty() {
            scope_spans.push(wrap_spans_in_scope(plartform_spans, function_context));
        }
        if scope_spans.is_empty() {
            Vec::new()
        } else {
            vec![wrap_spans_in_resource(
                scope_spans,
                function_context,
                &self.config.processor_config.resource_attributes.traces,
            )]
        }
    }

    fn prepare_metrics(
        &self,
        state: &mut TelemetryServiceState,
        function_context: &FunctionContext,
    ) -> Vec<ResourceMetrics> {
        let mut scope_metrics: Vec<ScopeMetrics> = Vec::new();
        if self.config.processor_config.platform_metrics_mode != PlatformMetricsMode::Disabled {
            let metrics = state.metrics_state.make_metrics_report();
            if !metrics.is_empty() {
                scope_metrics.push(wrap_metrics_in_scope(metrics));
            }
        };
        match self.config.processor_config.otel_metrics_mode {
            OtelMetricsMode::Disabled => (),
            OtelMetricsMode::Direct => {
                scope_metrics.extend(
                    take(&mut state.output_buffers.function_metrics_buffer)
                        .into_iter()
                        .flat_map(|rm| rm.scope_metrics.into_iter())
                        .filter(|m| !m.metrics.is_empty()),
                );
            }
            OtelMetricsMode::Processed => {
                let metrics = state.metrics_accumulator.export();
                if !metrics.is_empty() {
                    scope_metrics.push(wrap_metrics_in_scope(metrics))
                }
            }
        };

        if scope_metrics.is_empty() {
            Vec::new()
        } else {
            vec![wrap_metrics_in_resource(
                scope_metrics,
                function_context,
                &self.config.processor_config.resource_attributes.metrics,
            )]
        }
    }

    fn prepare_epsagon_traces(&self, state: &mut TelemetryServiceState) -> Vec<String> {
        take(&mut state.output_buffers.epsagon_traces_buffer)
    }
}
