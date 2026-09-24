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

use crate::coralogix::OtlpExportResponse;
use crate::coralogix::{self, sending_verb};
use crate::proto::opentelemetry::proto::logs::v1::ResourceLogs;
use crate::proto::opentelemetry::proto::metrics::v1::ResourceMetrics;
use crate::proto::opentelemetry::proto::trace::v1::ResourceSpans;
use crate::telemetry::telemetry_sender::{
    BatchTelemetrySender, DynPillarTelemetrySender, ItemCount, PillarTelemetrySender,
    TelemetryBatch,
};
use async_trait::async_trait;
use futures::join;
use std::mem::take;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{debug, error, trace, warn};

use super::DynOtlpSender;

pub struct CoralogixTelemetrySender {
    pub logs_telemetry_sender: DynPillarTelemetrySender<ResourceLogs>,
    pub spans_telemetry_sender: DynPillarTelemetrySender<ResourceSpans>,
    pub metrics_telemetry_sender: DynPillarTelemetrySender<ResourceMetrics>,
    pub epsagon_traces_telemetry_sender: DynEpsagonTracesTelemetrySender,
}

#[async_trait]
impl BatchTelemetrySender for CoralogixTelemetrySender {
    async fn send_telemetry(&self, batch: TelemetryBatch) {
        debug!("Sending telemetry to Coralogix.");

        join!(
            self.logs_telemetry_sender.send_telemetry(batch.logs),
            self.spans_telemetry_sender.send_telemetry(batch.spans),
            self.metrics_telemetry_sender.send_telemetry(batch.metrics),
            self.epsagon_traces_telemetry_sender
                .send_telemetry(batch.epsagon_traces)
        );
    }
}

pub struct OtlpPillarTelemetrySender<T> {
    pillar: &'static str,
    sender: DynOtlpSender<Vec<T>>,
    state: Arc<Mutex<RetryState<T>>>,
}

#[derive(Debug)]
struct RetryState<T> {
    to_retry: Option<Vec<T>>,
}

#[async_trait]
impl<T> PillarTelemetrySender<T> for OtlpPillarTelemetrySender<T>
where
    T: ItemCount + Clone + Send,
{
    async fn send_telemetry(&self, to_send: Vec<T>) {
        let to_retry = {
            let mut state = self.state.lock().await;
            take(&mut state.to_retry)
        };

        let (retry_result, result) = join!(
            self.resend(to_retry.clone()),
            self.send(to_send.clone(), false),
        );

        {
            let mut state = self.state.lock().await;

            // coralogix::Error has to_string that is better than debug
            if let Err(error) = retry_result {
                error!(%error, "Failed to resend {} to Coralogix.", self.pillar);
            }

            if let Err(error) = result {
                if error.is_retryable() {
                    warn!(
                        %error,
                        "Failed to send {} to Coralogix. Will retry.", self.pillar
                    );
                    state.to_retry = Some(to_send);
                } else {
                    warn!(
                        %error,
                        "Failed to send {} to Coralogix. No retry will be attempted for this type of error.", self.pillar
                    );
                }
            }
        }
    }
}

impl<T> OtlpPillarTelemetrySender<T>
where
    T: ItemCount,
{
    pub fn new(
        pillar: &'static str,
        sender: DynOtlpSender<Vec<T>>,
    ) -> OtlpPillarTelemetrySender<T> {
        OtlpPillarTelemetrySender {
            pillar,
            sender,
            state: Arc::new(Mutex::new(RetryState { to_retry: None })),
        }
    }

    async fn resend(&self, t: Option<Vec<T>>) -> Result<(), coralogix::Error> {
        if let Some(t) = t {
            self.send(t, true).await?;
        }
        Ok(())
    }

    async fn send(&self, t: Vec<T>, is_retry: bool) -> Result<(), coralogix::Error> {
        let item_count = t.item_count();
        if item_count == 0 {
            return Ok(());
        }
        trace!(
            "{} {} {} (in {} resources)",
            sending_verb(is_retry),
            item_count,
            self.pillar,
            t.resource_count()
        );
        let t0 = Instant::now();
        let response = self.sender.send(t).await?;
        let latency_ms = t0.elapsed().as_millis();

        match response {
            OtlpExportResponse::Success => debug!(
                "Successfully delivered {} to Coralogix in {}ms",
                self.pillar, latency_ms
            ),
            OtlpExportResponse::Warning { message } => warn!(
                "Warning regarding {} received from Coralogix: {}",
                self.pillar, message
            ),
            OtlpExportResponse::PartialSuccess {
                message,
                dropped_items,
            } => error!(
                "{} {} were rejected by Coralogix: {}",
                dropped_items, self.pillar, message
            ),
        }
        Ok(())
    }
}

#[async_trait]
pub trait EpsagonTracesTelemetrySender {
    async fn send_telemetry(&self, to_send: Vec<String>);
}

pub type DynEpsagonTracesTelemetrySender = Arc<dyn EpsagonTracesTelemetrySender + Send + Sync>;

pub struct NoopEpsagonTracesTelemetrySender {}

#[async_trait]
impl EpsagonTracesTelemetrySender for NoopEpsagonTracesTelemetrySender {
    async fn send_telemetry(&self, _: Vec<String>) {}
}
