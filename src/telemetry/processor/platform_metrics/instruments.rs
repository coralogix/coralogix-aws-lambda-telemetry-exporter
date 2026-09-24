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

use crate::proto::opentelemetry::proto::metrics::v1 as otel_metrics;
use crate::proto::opentelemetry::proto::metrics::v1::Metric;
use crate::proto::opentelemetry::proto::metrics::v1::NumberDataPoint;
use crate::proto::opentelemetry::proto::metrics::v1::SummaryDataPoint;

use super::{MAX_TIME_BETWEEN_DATA_NANOS, REPORT_LAST_TIME_NANOS};

#[derive(Debug, Clone)]
pub struct Descriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub unit: &'static str,
}

#[derive(Debug, Clone)]
pub struct Counter {
    descriptor: Descriptor,
    count: i64,
    last_count: i64,
    last_report_timestamp_nanos: u64,
}

impl Counter {
    pub fn new(descriptor: Descriptor) -> Self {
        Self {
            descriptor,
            count: 0,
            last_count: 0,
            last_report_timestamp_nanos: 0,
        }
    }

    pub fn record(&mut self, value: i64) {
        self.count += value;
    }

    pub fn report(&mut self, start_timestamp_nanos: u64, timestamp_nanos: u64) -> Option<Metric> {
        if self.count != self.last_count {
            let mut v: Vec<NumberDataPoint> = Vec::new();
            if timestamp_nanos > self.last_report_timestamp_nanos + MAX_TIME_BETWEEN_DATA_NANOS {
                v.push(NumberDataPoint {
                    attributes: Vec::new(),
                    start_time_unix_nano: start_timestamp_nanos,
                    time_unix_nano: timestamp_nanos - REPORT_LAST_TIME_NANOS,
                    exemplars: vec![],
                    flags: 0,
                    value: Some(otel_metrics::number_data_point::Value::AsInt(
                        self.last_count,
                    )),
                });
            }
            v.push(otel_metrics::NumberDataPoint {
                attributes: Vec::new(),
                // "where usually start means a process/application start" (https://opentelemetry.io/docs/reference/specification/metrics/data-model/#sums)
                start_time_unix_nano: start_timestamp_nanos,
                time_unix_nano: timestamp_nanos,
                exemplars: vec![], // TODO would be nice in the future
                flags: 0,
                value: Some(otel_metrics::number_data_point::Value::AsInt(self.count)),
            });
            self.last_count = self.count;
            self.last_report_timestamp_nanos = timestamp_nanos;

            Some(Metric {
                name: self.descriptor.name.to_owned(),
                description: self.descriptor.description.to_owned(),
                unit: self.descriptor.unit.to_owned(),
                data: Some(otel_metrics::metric::Data::Sum(otel_metrics::Sum {
                    aggregation_temporality: otel_metrics::AggregationTemporality::Cumulative
                        as i32,
                    is_monotonic: true,
                    data_points: v,
                })),
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct Summary {
    descriptor: Descriptor,
    sum: f64,
    count: u64,
    last_sum: f64,
    last_count: u64,
    last_report_timestamp_nanos: u64,
}

impl Summary {
    pub fn new(descriptor: Descriptor) -> Self {
        Self {
            descriptor,
            sum: 0.0,
            count: 0,
            last_sum: 0.0,
            last_count: 0,
            last_report_timestamp_nanos: 0,
        }
    }

    pub fn record(&mut self, value: f64) {
        self.sum += value;
        self.count += 1;
    }

    pub fn report(&mut self, start_timestamp_nanos: u64, timestamp_nanos: u64) -> Option<Metric> {
        if self.count != self.last_count {
            let mut v: Vec<SummaryDataPoint> = Vec::new();
            if timestamp_nanos > self.last_report_timestamp_nanos + MAX_TIME_BETWEEN_DATA_NANOS {
                v.push(SummaryDataPoint {
                    attributes: Vec::new(),
                    start_time_unix_nano: start_timestamp_nanos,
                    time_unix_nano: timestamp_nanos - REPORT_LAST_TIME_NANOS,
                    flags: 0,
                    count: self.last_count,
                    sum: self.last_sum,
                    quantile_values: vec![],
                });
            }
            // TODO Summary works well here, but it is marked as "Legacy" in OTEL documentation. https://opentelemetry.io/docs/reference/specification/metrics/data-model/#summary-legacy
            // In the end its basically two Sums (because we don't report quantile_values)
            v.push(SummaryDataPoint {
                attributes: Vec::new(),
                start_time_unix_nano: start_timestamp_nanos,
                time_unix_nano: timestamp_nanos,
                flags: 0,
                count: self.count,
                sum: self.sum,
                quantile_values: vec![],
            });
            self.last_sum = self.sum;
            self.last_count = self.count;
            self.last_report_timestamp_nanos = timestamp_nanos;

            Some(Metric {
                name: self.descriptor.name.to_owned(),
                description: self.descriptor.description.to_owned(),
                unit: self.descriptor.unit.to_owned(),
                data: Some(otel_metrics::metric::Data::Summary(otel_metrics::Summary {
                    data_points: v,
                })),
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct MaxGauge {
    descriptor: Descriptor,
    max: Option<i64>,
}

impl MaxGauge {
    pub fn new(descriptor: Descriptor) -> Self {
        Self {
            descriptor,
            max: None,
        }
    }

    pub fn record(&mut self, value: i64) {
        self.max = match self.max {
            Some(current) => Some(std::cmp::max(current, value)),
            None => Some(value),
        }
    }

    pub fn report(&mut self, start_timestamp_nanos: u64, timestamp_nanos: u64) -> Option<Metric> {
        self.max.map(|value| {
            // Unlike counter or timer max gauge is reported always, and without reporting previous value. (because gauges aren't used with `rate` operator)
            let v = vec![NumberDataPoint {
                attributes: Vec::new(),
                // "This is commonly set to the timestamp when a metric collection system started." https://opentelemetry.io/docs/reference/specification/metrics/data-model/#gauge
                start_time_unix_nano: start_timestamp_nanos,
                time_unix_nano: timestamp_nanos,
                exemplars: vec![],
                flags: 0,
                value: Some(otel_metrics::number_data_point::Value::AsInt(value)),
            }];

            Metric {
                name: self.descriptor.name.to_owned(),
                description: self.descriptor.description.to_owned(),
                unit: self.descriptor.unit.to_owned(),
                data: Some(otel_metrics::metric::Data::Gauge(otel_metrics::Gauge {
                    data_points: v,
                })),
            }
        })
    }
}
