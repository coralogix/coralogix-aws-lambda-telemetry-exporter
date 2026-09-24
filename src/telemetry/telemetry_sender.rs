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

use crate::proto::opentelemetry::proto::logs::v1::ResourceLogs;
use crate::proto::opentelemetry::proto::metrics::v1::ResourceMetrics;
use crate::proto::opentelemetry::proto::trace::v1::ResourceSpans;
use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use std::mem::take;
use std::sync::Arc;

pub type DynPillarTelemetrySender<T> = Arc<dyn PillarTelemetrySender<T> + Send + Sync>;

#[async_trait]
pub trait PillarTelemetrySender<T> {
    async fn send_telemetry(&self, to_send: Vec<T>);
}

#[derive(Default, Debug, Clone)]
pub struct TelemetryBatch {
    pub logs: Vec<ResourceLogs>,
    pub spans: Vec<ResourceSpans>,
    pub metrics: Vec<ResourceMetrics>,
    pub epsagon_traces: Vec<String>,
}

pub type DynBatchTelemetrySender = Arc<dyn BatchTelemetrySender + Send + Sync>;

#[async_trait]
pub trait BatchTelemetrySender {
    // send_telemetry is meant to be infallible. If sending fails, the implementation is responsible for logging the failure and optionally scheduling a retry
    async fn send_telemetry(&self, batch: TelemetryBatch);
}

pub struct CompositeTelemetrySender {
    pub logs: Option<DynPillarTelemetrySender<ResourceLogs>>,
    pub traces: Option<DynPillarTelemetrySender<ResourceSpans>>,
    pub metrics: Option<DynPillarTelemetrySender<ResourceMetrics>>,
    pub main: Option<DynBatchTelemetrySender>,
}

#[async_trait]
impl BatchTelemetrySender for CompositeTelemetrySender {
    async fn send_telemetry(&self, mut batch: TelemetryBatch) {
        let futures = FuturesUnordered::new();

        if let Some(logs_sender) = self.logs.as_ref() {
            let logs = take(&mut batch.logs);
            futures.push(logs_sender.send_telemetry(logs));
        }

        if let Some(traces_sender) = self.traces.as_ref() {
            let spans = take(&mut batch.spans);
            futures.push(traces_sender.send_telemetry(spans));
        }

        if let Some(metrics_sender) = self.metrics.as_ref() {
            let metrics = take(&mut batch.metrics);
            futures.push(metrics_sender.send_telemetry(metrics));
        }

        if let Some(main_sender) = self.main.as_ref() {
            let f = main_sender.send_telemetry(batch);
            futures.push(f);
        }

        futures.collect::<Vec<()>>().await;
    }
}

pub trait ItemCount {
    fn item_count(&self) -> usize;
    fn resource_count(&self) -> usize;
}

impl<T> ItemCount for Vec<T>
where
    T: ItemCount,
{
    fn item_count(&self) -> usize {
        self.iter().map(|item| item.item_count()).sum()
    }

    fn resource_count(&self) -> usize {
        self.iter().map(|item| item.resource_count()).sum()
    }
}

impl ItemCount for ResourceLogs {
    fn item_count(&self) -> usize {
        self.scope_logs.iter().map(|sl| sl.log_records.len()).sum()
    }

    fn resource_count(&self) -> usize {
        self.scope_logs.len()
    }
}

impl ItemCount for ResourceSpans {
    fn item_count(&self) -> usize {
        self.scope_spans.iter().map(|sl| sl.spans.len()).sum()
    }

    fn resource_count(&self) -> usize {
        self.scope_spans.len()
    }
}

impl ItemCount for ResourceMetrics {
    fn item_count(&self) -> usize {
        self.scope_metrics.iter().map(|sl| sl.metrics.len()).sum()
    }

    fn resource_count(&self) -> usize {
        self.scope_metrics.len()
    }
}
