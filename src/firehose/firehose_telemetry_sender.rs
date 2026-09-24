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
use crate::proto::com::coralogix::ingress::otlp::v1::Telemetry;
use crate::telemetry::telemetry_sender::{BatchTelemetrySender, ItemCount, TelemetryBatch};
use async_trait::async_trait;
use futures::join;
use std::mem::take;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{debug, error, trace, warn};

use super::firehose_sender::DynFirehoseSender;

pub struct FirehoseTelemetrySender {
    firehose: DynFirehoseSender,
    state: Arc<Mutex<FirehoseTelemetrySenderState>>,
}

#[derive(Debug, Default)]
struct FirehoseTelemetrySenderState {
    telemetry_to_retry: Option<Telemetry>,
}

#[async_trait]
impl BatchTelemetrySender for FirehoseTelemetrySender {
    async fn send_telemetry(&self, batch: TelemetryBatch) {
        trace!("Sending telemetry to coralogix.");

        let telemetry_to_send = Telemetry {
            logs: batch.logs,
            spans: batch.spans,
            metrics: batch.metrics,
            epsagon_traces: batch.epsagon_traces,
        };

        let telemetry_to_retry = {
            let mut state = self.state.lock().await;
            take(&mut state.telemetry_to_retry)
        };

        let (retry_result, result) = join!(
            self.resend(telemetry_to_retry.clone()),
            self.send(telemetry_to_send.clone(), false),
        );

        {
            let mut state = self.state.lock().await;

            // firehose::Error has to_string that is better than debug
            if let Err(error) = retry_result {
                error!(error, "Failed to resend telemetry to Coralogix.");
            }

            if let Err(error) = result {
                warn!(error, "Failed to send telemetry to Coralogix. Will retry.");
                state.telemetry_to_retry = Some(telemetry_to_send);
            }
        }
    }
}

impl FirehoseTelemetrySender {
    pub fn new(firehose: DynFirehoseSender) -> FirehoseTelemetrySender {
        FirehoseTelemetrySender {
            firehose,
            state: Arc::new(Mutex::new(FirehoseTelemetrySenderState::default())),
        }
    }

    async fn resend(&self, telemetry: Option<Telemetry>) -> Result<(), Error> {
        if let Some(telemetry) = telemetry {
            self.send(telemetry, true).await?;
        }
        Ok(())
    }

    async fn send(&self, telemetry: Telemetry, is_retry: bool) -> Result<(), Error> {
        let log_count = telemetry.logs.item_count();
        let span_count = telemetry.spans.item_count();
        let metric_count = telemetry.metrics.item_count();
        let epsagon_trace_count = telemetry.epsagon_traces.len();

        if log_count != 0 || span_count != 0 || metric_count != 0 || epsagon_trace_count != 0 {
            trace!(
                "{} telemetry to Firehose. {} logs (in {} resources), {} spans (in {} resources), {} metrics (in {} resources), {} epsagon traces",
                sending_verb(is_retry),
                log_count,
                telemetry.logs.len(),
                span_count,
                telemetry.spans.len(),
                metric_count,
                telemetry.metrics.len(),
                epsagon_trace_count
            );
            let t0 = Instant::now();
            self.firehose.send(telemetry).await?;
            let latency_ms = t0.elapsed().as_millis();
            debug!(
                "Successfully delivered telemetry to Firehose in {}ms",
                latency_ms
            );
        }
        Ok(())
    }
}

fn sending_verb(is_retry: bool) -> &'static str {
    if is_retry { "Resending" } else { "Sending" }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use async_trait::async_trait;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Mutex;

    use super::FirehoseTelemetrySender;
    use crate::firehose::Error;
    use crate::firehose::firehose_sender::FirehoseSender;
    use crate::proto::com::coralogix::ingress::otlp::v1::Telemetry;
    use crate::proto::opentelemetry::proto::common::v1::AnyValue;
    use crate::proto::opentelemetry::proto::common::v1::any_value::Value;
    use crate::proto::opentelemetry::proto::logs::v1::{LogRecord, ResourceLogs, ScopeLogs};
    use crate::telemetry::telemetry_sender::{BatchTelemetrySender, TelemetryBatch};

    struct MockFirehoseSender {
        pub should_fail: Arc<AtomicBool>,
        pub sent: Mutex<Vec<Telemetry>>,
        pub failed: Mutex<Vec<Telemetry>>,
    }

    #[async_trait]
    impl FirehoseSender for MockFirehoseSender {
        async fn send(&self, telemetry: Telemetry) -> Result<(), Error> {
            if self.should_fail.load(Ordering::Relaxed) {
                self.failed.lock().await.push(telemetry);
                Err(Error::Internal(anyhow!("error")))
            } else {
                self.sent.lock().await.push(telemetry);
                Ok(())
            }
        }
    }

    impl MockFirehoseSender {
        fn new() -> MockFirehoseSender {
            MockFirehoseSender {
                should_fail: Arc::new(AtomicBool::new(false)),
                sent: Mutex::new(Vec::new()),
                failed: Mutex::new(Vec::new()),
            }
        }
    }

    fn test_logs(s: String) -> Vec<ResourceLogs> {
        vec![ResourceLogs {
            scope_logs: vec![ScopeLogs {
                log_records: vec![LogRecord {
                    body: as_log_body(s),
                    ..LogRecord::default()
                }],
                ..ScopeLogs::default()
            }],
            ..ResourceLogs::default()
        }]
    }

    fn as_log_body(s: String) -> Option<AnyValue> {
        Some(AnyValue {
            value: Some(Value::StringValue(s)),
        })
    }

    fn flatten_logs(telemetry: &[Telemetry]) -> Vec<Option<&AnyValue>> {
        telemetry
            .iter()
            .flat_map(|t| t.logs.iter())
            .flat_map(|rl| rl.scope_logs.iter())
            .flat_map(|sl| sl.log_records.iter())
            .map(|lr| lr.body.as_ref())
            .collect()
    }

    async fn send_log(telemetry_sender: &FirehoseTelemetrySender, log: &str) {
        telemetry_sender
            .send_telemetry(TelemetryBatch {
                logs: test_logs(log.to_owned()),
                ..Default::default()
            })
            .await
    }

    #[tokio::test]
    async fn should_retry_once() {
        let sender = Arc::new(MockFirehoseSender::new());
        let telemetry_sender = FirehoseTelemetrySender::new(sender.clone());

        sender.should_fail.store(true, Ordering::Relaxed);
        let _ = send_log(&telemetry_sender, "test log 1").await; // discarding the error

        // sending the log has failed
        assert_eq!(sender.sent.lock().await.len(), 0);

        sender.should_fail.store(false, Ordering::Relaxed);
        let _ = send_log(&telemetry_sender, "test log 2").await;

        // the new log has been sent and the old one has been resent
        assert_eq!(sender.sent.lock().await.len(), 2);
        let sent_logs = sender.sent.lock().await.clone();
        assert_eq!(
            flatten_logs(&sent_logs),
            vec![
                as_log_body("test log 1".to_owned()).as_ref(),
                as_log_body("test log 2".to_owned()).as_ref()
            ]
        )
    }

    #[tokio::test]
    async fn should_give_up_after_one_retry() {
        let sender = Arc::new(MockFirehoseSender::new());
        let telemetry_sender = FirehoseTelemetrySender::new(sender.clone());

        sender.should_fail.store(true, Ordering::Relaxed);
        let _ = send_log(&telemetry_sender, "test log 1").await; // discarding the error
        let _ = send_log(&telemetry_sender, "test log 2").await; // discarding the error

        // both attempts to send logs have failed
        assert_eq!(sender.sent.lock().await.len(), 0);

        sender.should_fail.store(false, Ordering::Relaxed);
        let _ = send_log(&telemetry_sender, "test log 3").await;

        // the log 3 has been sent and the log 2 retried, but log 1 is lost because it failed twice
        assert_eq!(sender.sent.lock().await.len(), 2);

        let sent_logs = sender.sent.lock().await.clone();
        assert_eq!(
            flatten_logs(&sent_logs),
            vec![
                as_log_body("test log 2".to_owned()).as_ref(),
                as_log_body("test log 3".to_owned()).as_ref()
            ]
        )
    }
}
