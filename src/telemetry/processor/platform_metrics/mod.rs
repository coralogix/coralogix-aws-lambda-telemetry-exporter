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

use crate::config::app_config::PlatformMetricsMode;
use crate::proto::opentelemetry::proto::metrics::v1 as otel_metrics;

pub mod instruments;
pub mod v1;
pub mod v2;

// If the time-distance between two data points of a counter is too long, Prometheus' `rate` will not produce any data.
// It is assumed that [1m] or longer interval will be used for querying these metrics.
// If a counter was last reported more than MAX_TIME_BETWEEN_DATA_NANOS ago, then the last value will be repeated before a new value is reported.
// That old value will be repeated with timestamp REPORT_LAST_TIME_NANOS before now.
pub const MAX_TIME_BETWEEN_DATA_NANOS: u64 = 35_000_000_000; // 35 seconds; seems reasonable given 1m querying interval.
pub const REPORT_LAST_TIME_NANOS: u64 = 30_000_000_000; // 30 seconds; Recommend by Nikita. Must be smaller than MAX_TIME_BETWEEN_DATA_NANOS

#[allow(clippy::large_enum_variant)]
pub enum PlatformMetricsState {
    Disabled,
    V1(v1::PlatformMetricsState),
    V2(v2::PlatformMetricsState),
}

impl PlatformMetricsState {
    pub fn for_mode(mode: PlatformMetricsMode) -> Self {
        match mode {
            PlatformMetricsMode::Disabled => Self::Disabled,
            PlatformMetricsMode::PlatformReport => Self::V1(v1::PlatformMetricsState::new()),
            PlatformMetricsMode::PlatformV2 => Self::V2(v2::PlatformMetricsState::new()),
        }
    }

    pub fn make_metrics_report(&mut self) -> Vec<otel_metrics::Metric> {
        match self {
            Self::Disabled => Vec::new(),
            Self::V1(state) => v1::make_metrics_report(state),
            Self::V2(state) => v2::make_metrics_report(state),
        }
    }
}
