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

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use std::mem::take;
use tokio::task::JoinHandle;
use tracing::debug;

use super::telemetry_sender::{DynBatchTelemetrySender, TelemetryBatch};

pub struct SendingSupervisor {
    telemetry_sender: DynBatchTelemetrySender,
    state: SendingSupervisorState,
}

impl std::fmt::Debug for SendingSupervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SendingSupervisor")
            .field("state", &self.state)
            .finish()
    }
}

#[derive(Debug)]
struct SendingSupervisorState {
    in_flight_send_operations: Vec<JoinHandle<()>>,
}

impl SendingSupervisor {
    pub fn new(telemetry_sender: DynBatchTelemetrySender) -> Self {
        SendingSupervisor {
            telemetry_sender,
            state: SendingSupervisorState {
                in_flight_send_operations: Vec::new(),
            },
        }
    }

    pub fn send(&mut self, batch: TelemetryBatch) {
        let telemetry_sender = self.telemetry_sender.clone();
        let handle = tokio::spawn(async move { telemetry_sender.send_telemetry(batch).await });
        self.state.in_flight_send_operations.push(handle);
    }

    pub async fn await_in_flight_sends(&mut self) {
        let mut in_flight: FuturesUnordered<JoinHandle<()>> =
            take(&mut self.state.in_flight_send_operations)
                .into_iter()
                .filter(|jh| !jh.is_finished())
                .collect();

        loop {
            if !in_flight.is_empty() {
                debug!(
                    "Awaiting {} in-flight telemetry requests before finishing.",
                    in_flight.len()
                );
            }
            if in_flight.next().await.is_none() {
                break;
            }
        }
    }
}
