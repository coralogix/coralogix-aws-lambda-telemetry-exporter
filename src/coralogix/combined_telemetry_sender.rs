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
use crate::proto::com::coralogix::ingress::otlp::v1::Telemetry;
use crate::telemetry::telemetry_sender::{BatchTelemetrySender, ItemCount, TelemetryBatch};
use async_trait::async_trait;
use futures::join;
use std::mem::take;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{debug, error, trace, warn};

use super::DynOtlpSender;

pub struct CombinedTelemetrySender {
    sender: DynOtlpSender<Telemetry>,
    state: Arc<Mutex<RetryState>>,
}

#[derive(Debug)]
struct RetryState {
    to_retry: Option<Telemetry>,
}

impl CombinedTelemetrySender {
    pub fn new(sender: DynOtlpSender<Telemetry>) -> Self {
        CombinedTelemetrySender {
            sender,
            state: Arc::new(Mutex::new(RetryState { to_retry: None })),
        }
    }
}

#[async_trait]
impl BatchTelemetrySender for CombinedTelemetrySender {
    async fn send_telemetry(&self, batch: TelemetryBatch) {
        debug!("Sending telemetry to Coralogix.");

        let to_send = Telemetry {
            logs: batch.logs,
            spans: batch.spans,
            metrics: batch.metrics,
            epsagon_traces: Vec::new(), // Not supported in this context
        };

        self.send_converted_telemetry(to_send).await;
    }
}

impl CombinedTelemetrySender {
    async fn send_converted_telemetry(&self, to_send: Telemetry) {
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
                error!(%error, "Failed to resend telemetry to Coralogix.");
            }

            if let Err(error) = result {
                if error.is_retryable() {
                    warn!(
                        %error,
                        "Failed to send telemetry to Coralogix. Will retry."
                    );
                    state.to_retry = Some(to_send);
                } else {
                    warn!(
                        %error,
                        "Failed to send telemetry to Coralogix. No retry will be attempted for this type of error."
                    );
                }
            }
        }
    }

    async fn resend(&self, t: Option<Telemetry>) -> Result<(), coralogix::Error> {
        if let Some(t) = t {
            self.send(t, true).await?;
        }
        Ok(())
    }

    async fn send(&self, t: Telemetry, is_retry: bool) -> Result<(), coralogix::Error> {
        let log_count = t.logs.item_count();
        let log_resources = t.logs.resource_count();
        let span_count = t.spans.item_count();
        let span_resources = t.spans.resource_count();
        let metric_count = t.metrics.item_count();
        let metric_resources = t.metrics.resource_count();

        if log_count == 0 && span_count == 0 && metric_count == 0 {
            return Ok(());
        }
        trace!(
            "{} {} logs in {} resources, {} spans in {} resources, {} metrics in {} resources",
            sending_verb(is_retry),
            log_count,
            log_resources,
            span_count,
            span_resources,
            metric_count,
            metric_resources
        );
        let t0 = Instant::now();
        let response = self.sender.send(t).await?;
        let latency_ms = t0.elapsed().as_millis();

        match response {
            OtlpExportResponse::Success => debug!(
                "Successfully delivered telemetry to Coralogix in {}ms",
                latency_ms
            ),
            OtlpExportResponse::Warning { message } => {
                warn!("Warning received from Coralogix: {}", message)
            }
            OtlpExportResponse::PartialSuccess {
                message,
                dropped_items,
            } => error!(
                "{} items were rejected by Coralogix: {}",
                dropped_items, message
            ),
        }
        Ok(())
    }
}
