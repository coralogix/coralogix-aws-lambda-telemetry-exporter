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

use std::borrow::Cow;
use std::collections::HashMap;
use std::mem::take;

use itertools::Itertools;

use crate::proto::opentelemetry::proto::common::v1::KeyValue;
use crate::proto::opentelemetry::proto::common::v1::any_value::Value;
use crate::proto::opentelemetry::proto::metrics::v1::{self as otel, AggregationTemporality};
use crate::proto::opentelemetry::proto::metrics::v1::{
    HistogramDataPoint, Metric, NumberDataPoint, SummaryDataPoint, metric,
};

use super::processor::{MAX_TIME_BETWEEN_DATA_NANOS, REPORT_LAST_TIME_NANOS};

const CUMULATIVE: i32 = AggregationTemporality::Cumulative as i32;

/*
* Unlike conventional non-FaaS services, Lambda instances cannot generate metrics samples at a regular interval.
*
* OtelMetricsAccumulator processed OTel metrics emitted by function code with two goals in mind:
* 1. To reduce the number of data points that are sent to the Coralogix backend.
* 2. To make sure that the data points that are sent to the Coralogix backend can be used in PromQL `sum(rate(ts[1m]))` queries`
*
* Gauges are not optimized. ExponentialHistograms are not supported (discarded).
* Sums, Histograms, and Summaries are optimized this way:
* Data points are discarded if they are identical to the previous data point.
* Whenever there is a gap before a data point larger than MAX_TIME_BETWEEN_DATA_NANOS,
* a new data point is injected with the same value as the last known data point,
* but with a timestamp that is REPORT_LAST_TIME_NANOS before the new data point.
* If the first data point is non-zero, a zero data point is injected before it.
* Ths makes PromQL `rate` work as expected.
*
* Resource and Scope are discarded as neither is actually used in the current implementation.
*/
#[derive(Default)]
pub struct OtelMetricsAccumulator {
    dropped_datapoints: u64,
    gauges: HashMap<String, MetricAccumulator<GaugeMetadata, NumberDataPoint>>,
    sums: HashMap<String, MetricAccumulator<SumMetadata, NumberDataPoint>>,
    histograms: HashMap<String, MetricAccumulator<HistogramMetadata, HistogramDataPoint>>,
    summaries: HashMap<String, MetricAccumulator<SummaryMetadata, SummaryDataPoint>>,
}

impl OtelMetricsAccumulator {
    pub fn accumulate(&mut self, metrics: Vec<Metric>) {
        metrics.into_iter().for_each(|m| self.accumulate_metric(m));
    }

    fn accumulate_metric(&mut self, m: Metric) {
        match m.data {
            Some(metric::Data::Gauge(g)) => {
                let metric_accumulator =
                    self.gauges
                        .entry(m.name.clone())
                        .or_insert_with(|| MetricAccumulator {
                            name: m.name.clone(),
                            description: m.description.clone(),
                            unit: m.unit.clone(),
                            metadata: GaugeMetadata {},
                            time_series: HashMap::new(),
                        });
                metric_accumulator.accumulate_as_is(g.data_points);
            }
            Some(metric::Data::Sum(s)) => {
                let metric_accumulator =
                    self.sums
                        .entry(m.name.clone())
                        .or_insert_with(|| MetricAccumulator {
                            name: m.name.clone(),
                            description: m.description.clone(),
                            unit: m.unit.clone(),
                            metadata: SumMetadata {
                                aggregation_temporality: s.aggregation_temporality,
                                is_monotonic: s.is_monotonic,
                            },
                            time_series: HashMap::new(),
                        });
                if s.aggregation_temporality == CUMULATIVE {
                    metric_accumulator.accumulate_if_different(s.data_points);
                } else {
                    metric_accumulator.accumulate_as_is(s.data_points);
                }
            }
            Some(metric::Data::Histogram(h)) => {
                let metric_accumulator =
                    self.histograms
                        .entry(m.name.clone())
                        .or_insert_with(|| MetricAccumulator {
                            name: m.name.clone(),
                            description: m.description.clone(),
                            unit: m.unit.clone(),
                            metadata: HistogramMetadata {
                                aggregation_temporality: h.aggregation_temporality,
                            },
                            time_series: HashMap::new(),
                        });
                if h.aggregation_temporality == CUMULATIVE {
                    metric_accumulator.accumulate_if_different(h.data_points);
                } else {
                    metric_accumulator.accumulate_as_is(h.data_points);
                }
            }
            Some(metric::Data::Summary(s)) => {
                let metric_accumulator =
                    self.summaries
                        .entry(m.name.clone())
                        .or_insert_with(|| MetricAccumulator {
                            name: m.name.clone(),
                            description: m.description.clone(),
                            unit: m.unit.clone(),
                            metadata: SummaryMetadata {},
                            time_series: HashMap::new(),
                        });
                metric_accumulator.accumulate_if_different(s.data_points);
            }
            Some(metric::Data::ExponentialHistogram(eh)) => {
                self.dropped_datapoints += eh.data_points.len() as u64;
            }
            None => (),
        };
    }

    pub fn export(&mut self) -> Vec<Metric> {
        self.gauges
            .iter_mut()
            .map(|(_, ma)| export_gauge(ma))
            .chain(self.sums.iter_mut().map(|(_, ma)| export_sum(ma)))
            .chain(
                self.histograms
                    .iter_mut()
                    .map(|(_, ma)| export_histogram(ma)),
            )
            .chain(self.summaries.iter_mut().map(|(_, ma)| export_summary(ma)))
            .collect_vec()
    }
}

fn export_gauge(ma: &mut MetricAccumulator<GaugeMetadata, NumberDataPoint>) -> Metric {
    let data_points = ma.take_as_is();
    make_metric(ma, metric::Data::Gauge(otel::Gauge { data_points }))
}

fn export_sum(ma: &mut MetricAccumulator<SumMetadata, NumberDataPoint>) -> Metric {
    let data_points = if ma.metadata.aggregation_temporality == CUMULATIVE {
        ma.take_inflated()
    } else {
        ma.take_as_is()
    };

    make_metric(
        ma,
        metric::Data::Sum(otel::Sum {
            data_points,
            aggregation_temporality: ma.metadata.aggregation_temporality,
            is_monotonic: ma.metadata.is_monotonic,
        }),
    )
}

fn export_histogram(ma: &mut MetricAccumulator<HistogramMetadata, HistogramDataPoint>) -> Metric {
    let data_points = if ma.metadata.aggregation_temporality == CUMULATIVE {
        ma.take_inflated()
    } else {
        ma.take_as_is()
    };

    make_metric(
        ma,
        metric::Data::Histogram(otel::Histogram {
            data_points,
            aggregation_temporality: ma.metadata.aggregation_temporality,
        }),
    )
}

fn export_summary(ma: &mut MetricAccumulator<SummaryMetadata, SummaryDataPoint>) -> Metric {
    let data_points = ma.take_inflated();
    make_metric(ma, metric::Data::Summary(otel::Summary { data_points }))
}

fn make_metric<M, DP>(ma: &MetricAccumulator<M, DP>, data: metric::Data) -> Metric {
    Metric {
        name: ma.name.clone(),
        description: ma.description.clone(),
        unit: ma.unit.clone(),
        data: Some(data),
    }
}

struct MetricAccumulator<M, DP> {
    pub name: String,
    pub description: String,
    pub unit: String,
    pub metadata: M,
    pub time_series: HashMap<TimeSeriesId, TimeSeries<DP>>,
}

impl<M, DP> MetricAccumulator<M, DP>
where
    DP: DataPoint,
{
    fn get_time_series(&mut self, id: TimeSeriesId) -> &mut TimeSeries<DP> {
        self.time_series.entry(id).or_insert_with(|| TimeSeries {
            previous_point: None,
            new_points: Vec::new(),
        })
    }

    fn accumulate_as_is(&mut self, dps: Vec<DP>)
    where
        DP: DataPoint,
    {
        dps.into_iter().for_each(|dp| {
            let ts = self.get_time_series(dp.time_series_id());
            ts.new_points.push(dp)
        })
    }

    fn accumulate_if_different(&mut self, dps: Vec<DP>)
    where
        DP: DataPoint,
    {
        dps.into_iter().for_each(|dp| {
            let ts = self.get_time_series(dp.time_series_id());

            let is_first_or_different = ts.last_point().is_none_or(|p| !p.is_equivalent(&dp));
            if is_first_or_different {
                ts.new_points.push(dp);
            }
        })
    }

    fn take_as_is(&mut self) -> Vec<DP>
    where
        DP: DataPoint,
    {
        self.time_series
            .iter_mut()
            .flat_map(|(_, ts)| take(&mut ts.new_points))
            .collect_vec()
    }

    fn take_inflated(&mut self) -> Vec<DP>
    where
        DP: DataPoint,
    {
        self.time_series
            .iter_mut()
            .flat_map(|(_, ts)| take_and_inflate_data_points(ts))
            .collect_vec()
    }
}

fn take_and_inflate_data_points<DP>(ts: &mut TimeSeries<DP>) -> Vec<DP>
where
    DP: DataPoint,
{
    let mut output = Vec::new();
    if ts.new_points.is_empty() {
        return output;
    }

    let (mut prev, iter) = if let Some(pp) = take(&mut ts.previous_point) {
        (Cow::Owned(pp), take(&mut ts.new_points).into_iter())
    } else {
        let mut iter = take(&mut ts.new_points).into_iter();
        // unwrap is protected by `if new_points.is_empty() ` check earlier
        let first = iter.next().unwrap();
        // We precede the first data point in the time series with a zero data point to ensure rate works as expected.
        if !first.is_zero() {
            let zero = first.to_zero(
                first
                    .get_time_unix_nano()
                    .saturating_sub(REPORT_LAST_TIME_NANOS),
            );
            output.push(zero);
        }
        output.push(first);
        // unwrap is safe because we just pushed
        (Cow::Borrowed(output.last().unwrap()), iter)
    };

    for this in iter {
        if this.get_time_unix_nano() > prev.get_time_unix_nano() + MAX_TIME_BETWEEN_DATA_NANOS {
            output.push(
                prev.into_owned().clone_with_timestamp(
                    this.get_time_unix_nano()
                        .saturating_sub(REPORT_LAST_TIME_NANOS),
                ),
            );
            output.push(this);
        } else {
            output.push(this);
        }
        // unwrap is safe because we just pushed
        prev = Cow::Borrowed(output.last().unwrap());
    }

    ts.previous_point = Some(output.last().unwrap().clone());

    output
}

struct GaugeMetadata {}

struct SumMetadata {
    pub aggregation_temporality: i32,
    pub is_monotonic: bool,
}

struct HistogramMetadata {
    pub aggregation_temporality: i32,
}

struct SummaryMetadata {}

struct TimeSeries<DP> {
    previous_point: Option<DP>,
    new_points: Vec<DP>,
}

impl<DP> TimeSeries<DP>
where
    DP: DataPoint,
{
    fn last_point(&self) -> Option<&DP> {
        self.new_points.last().or(self.previous_point.as_ref())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct TimeSeriesId {
    attributes: Vec<(String, Option<String>)>,
}

impl From<Vec<KeyValue>> for TimeSeriesId {
    fn from(attributes: Vec<KeyValue>) -> Self {
        TimeSeriesId {
            attributes: attributes
                .into_iter()
                .map(|kv| {
                    (
                        kv.key,
                        kv.value.and_then(|av| av.value).map(value_to_string),
                    )
                })
                .sorted() // The attributes need to be sorted to ensure that the order of the attributes does not matter for comparison of TimeSeriesIds
                .collect_vec(),
        }
    }
}

// metrics-gateway is going to reduce attribute values into strings anyway. We do it here, earlier, with the same logic, so that the value can be used as in a key in a HashMap.
fn value_to_string(any_value: Value) -> String {
    match any_value {
        Value::StringValue(v) => v,
        Value::BoolValue(v) => v.to_string(),
        Value::IntValue(v) => v.to_string(),
        Value::DoubleValue(v) => v.to_string(),
        Value::ArrayValue(v) => v
            .values
            .into_iter()
            .flat_map(|av| av.value.map(value_to_string))
            .join(","),
        Value::KvlistValue(v) => v
            .values
            .into_iter()
            .flat_map(|kv| {
                kv.value.and_then(|v| {
                    v.value
                        .map(|v| format!("{}:{}", kv.key, value_to_string(v)))
                })
            })
            .join(","),
        Value::BytesValue(b) => std::str::from_utf8(&b).unwrap_or("").to_owned(),
    }
}

trait DataPoint: Clone {
    fn get_time_unix_nano(&self) -> u64;

    fn clone_with_timestamp(&self, time_unix_nano: u64) -> Self;

    // Comparison that only checks the attributes that we care about
    fn is_equivalent(&self, other: &Self) -> bool;

    fn time_series_id(&self) -> TimeSeriesId;

    fn is_zero(&self) -> bool;

    fn to_zero(&self, time_unix_nano: u64) -> Self;
}

impl DataPoint for NumberDataPoint {
    fn get_time_unix_nano(&self) -> u64 {
        self.time_unix_nano
    }

    fn clone_with_timestamp(&self, time_unix_nano: u64) -> Self {
        NumberDataPoint {
            time_unix_nano,
            ..self.clone()
        }
    }

    fn is_equivalent(&self, other: &Self) -> bool {
        self.value == other.value && self.flags == other.flags
    }

    fn time_series_id(&self) -> TimeSeriesId {
        TimeSeriesId::from(self.attributes.clone())
    }

    fn is_zero(&self) -> bool {
        match &self.value {
            Some(otel::number_data_point::Value::AsInt(i)) => *i == 0,
            Some(otel::number_data_point::Value::AsDouble(d)) => *d == 0.0,
            None => true,
        }
    }

    fn to_zero(&self, time_unix_nano: u64) -> Self {
        let value = match &self.value {
            Some(otel::number_data_point::Value::AsInt(_)) => {
                Some(otel::number_data_point::Value::AsInt(0))
            }
            Some(otel::number_data_point::Value::AsDouble(_)) => {
                Some(otel::number_data_point::Value::AsDouble(0.0))
            }
            None => None,
        };
        NumberDataPoint {
            value,
            time_unix_nano,
            ..self.clone()
        }
    }
}

impl DataPoint for HistogramDataPoint {
    fn get_time_unix_nano(&self) -> u64 {
        self.time_unix_nano
    }

    fn clone_with_timestamp(&self, time_unix_nano: u64) -> Self {
        HistogramDataPoint {
            time_unix_nano,
            ..self.clone()
        }
    }

    fn is_equivalent(&self, other: &Self) -> bool {
        self.bucket_counts == other.bucket_counts
            && self.explicit_bounds == other.explicit_bounds
            && self.sum == other.sum
            && self.count == other.count
            && self.min == other.min
            && self.max == other.max
            && self.flags == other.flags
    }

    fn time_series_id(&self) -> TimeSeriesId {
        TimeSeriesId::from(self.attributes.clone())
    }

    fn is_zero(&self) -> bool {
        self.sum.is_none_or(|x| x == 0.0)
            && self.count == 0
            && self.min.is_none_or(|x| x == 0.0)
            && self.max.is_none_or(|x| x == 0.0)
            && self.bucket_counts.iter().all(|c| *c == 0)
    }

    fn to_zero(&self, time_unix_nano: u64) -> Self {
        HistogramDataPoint {
            sum: self.sum.map(|_| 0.0),
            count: 0,
            min: self.min.map(|_| 0.0),
            max: self.max.map(|_| 0.0),
            bucket_counts: self.bucket_counts.iter().map(|_| 0).collect(),
            time_unix_nano,
            ..self.clone()
        }
    }
}

impl DataPoint for SummaryDataPoint {
    fn get_time_unix_nano(&self) -> u64 {
        self.time_unix_nano
    }

    fn clone_with_timestamp(&self, time_unix_nano: u64) -> Self {
        SummaryDataPoint {
            time_unix_nano,
            ..self.clone()
        }
    }

    fn is_equivalent(&self, other: &Self) -> bool {
        self.quantile_values == other.quantile_values
            && self.sum == other.sum
            && self.count == other.count
            && self.flags == other.flags
    }

    fn time_series_id(&self) -> TimeSeriesId {
        TimeSeriesId::from(self.attributes.clone())
    }

    fn is_zero(&self) -> bool {
        self.sum == 0.0 && self.count == 0 && self.quantile_values.iter().all(|q| q.value == 0.0)
    }

    fn to_zero(&self, time_unix_nano: u64) -> Self {
        SummaryDataPoint {
            sum: 0.0,
            count: 0,
            quantile_values: self
                .quantile_values
                .iter()
                .map(|q| otel::summary_data_point::ValueAtQuantile {
                    quantile: q.quantile,
                    value: 0.0,
                })
                .collect(),
            time_unix_nano,
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod test {

    use itertools::Itertools;

    use crate::proto::opentelemetry::proto::common::v1::any_value::Value;
    use crate::proto::opentelemetry::proto::common::v1::{AnyValue, KeyValue};
    use crate::proto::opentelemetry::proto::metrics::v1::metric::Data;
    use crate::proto::opentelemetry::proto::metrics::v1::{self as otel, number_data_point};
    use crate::proto::opentelemetry::proto::metrics::v1::{AggregationTemporality, Metric};
    use crate::proto::opentelemetry::proto::metrics::v1::{NumberDataPoint, metric};
    use crate::telemetry::otel_metrics::OtelMetricsAccumulator;

    // I realize that using `&'static str` as a function argument is "unorthodox", but I think it makes sense as a tool to make test scenarios cleaner.
    fn make_cumulative_sum_metric(name: &'static str, data_points: Vec<NumberDataPoint>) -> Metric {
        Metric {
            name: name.to_owned(),
            description: "".to_owned(),
            unit: "".to_owned(),
            data: Some(metric::Data::Sum(otel::Sum {
                aggregation_temporality: AggregationTemporality::Cumulative as i32,
                is_monotonic: true,
                data_points,
            })),
        }
    }

    fn make_number_time_series(
        attributes: Vec<(&'static str, &'static str)>,
        points: Vec<(u64, i64)>,
    ) -> Vec<NumberDataPoint> {
        points
            .into_iter()
            .map(|(time, value)| make_number_point(attributes.clone(), time, value))
            .collect()
    }

    fn make_number_point(
        attributes: Vec<(&'static str, &'static str)>,
        time: u64,
        value: i64,
    ) -> NumberDataPoint {
        NumberDataPoint {
            start_time_unix_nano: 0,
            time_unix_nano: time,
            value: Some(number_data_point::Value::AsInt(value)),
            attributes: make_attributes(attributes),
            exemplars: vec![],
            flags: 0,
        }
    }

    fn make_attributes(attributes: Vec<(&'static str, &'static str)>) -> Vec<KeyValue> {
        attributes
            .into_iter()
            .map(|(k, v)| KeyValue {
                key: k.to_owned(),
                value: Some(AnyValue {
                    value: Some(Value::StringValue(v.to_owned())),
                }),
            })
            .collect()
    }

    fn assert_one_metric(metrics: Vec<Metric>) -> Metric {
        match &metrics[..] {
            [m] => m.clone(),
            [] => panic!("Expected one metric, got no metrics!"),
            _ => panic!(
                "Expected one metric, got {} metrics with names: {:?}",
                metrics.len(),
                metrics.into_iter().map(|m| m.name).collect_vec()
            ),
        }
    }

    fn assert_sum(metric: Metric) -> Vec<NumberDataPoint> {
        match metric.data {
            Some(Data::Sum(sum)) => sum.data_points,
            other => panic!("Expected Sum, got {:?}", other),
        }
    }

    fn assert_gauge(metric: Metric) -> Vec<NumberDataPoint> {
        match metric.data {
            Some(Data::Gauge(gauge)) => gauge.data_points,
            other => panic!("Expected Gauge, got {:?}", other),
        }
    }

    fn assert_int(point: &NumberDataPoint) -> i64 {
        match &point.value {
            Some(number_data_point::Value::AsInt(i)) => *i,
            other => panic!("Expected AsInt, got {:?}", other),
        }
    }

    #[test]
    fn initial_zero_point_is_propagated_unchanged() {
        let mut acc = OtelMetricsAccumulator::default();

        let metrics = vec![make_cumulative_sum_metric(
            "test",
            make_number_time_series(vec![], vec![(30_000_000_000, 0)]),
        )];

        acc.accumulate(metrics);
        let metrics = acc.export();
        let data_points = assert_sum(assert_one_metric(metrics));
        assert_eq!(data_points.len(), 1);
        assert_eq!(data_points[0].time_unix_nano, 30_000_000_000);
        assert_eq!(assert_int(&data_points[0]), 0);
    }

    #[test]
    fn non_zero_point_is_preceded_with_zero() {
        let mut acc = OtelMetricsAccumulator::default();

        let metrics = vec![make_cumulative_sum_metric(
            "test",
            make_number_time_series(vec![], vec![(30_000_000_000, 7)]),
        )];

        acc.accumulate(metrics);
        let metrics = acc.export();
        let data_points = assert_sum(assert_one_metric(metrics));
        assert_eq!(data_points.len(), 2);
        assert_eq!(data_points[0].time_unix_nano, 0);
        assert_eq!(assert_int(&data_points[0]), 0);
        assert_eq!(data_points[1].time_unix_nano, 30_000_000_000);
        assert_eq!(assert_int(&data_points[1]), 7);
    }

    #[test]
    fn two_equal_points_get_reduced_to_one() {
        let mut acc = OtelMetricsAccumulator::default();

        let metrics = vec![make_cumulative_sum_metric(
            "test",
            make_number_time_series(vec![], vec![(30_000_000_000, 7), (45_000_000_000, 7)]),
        )];

        acc.accumulate(metrics);
        let metrics = acc.export();
        let data_points = assert_sum(assert_one_metric(metrics));
        assert_eq!(data_points.len(), 2);
        assert_eq!(data_points[0].time_unix_nano, 0);
        assert_eq!(assert_int(&data_points[0]), 0);
        assert_eq!(data_points[1].time_unix_nano, 30_000_000_000);
        assert_eq!(assert_int(&data_points[1]), 7);
    }

    #[test]
    fn two_different_points_are_propagated_unchanged() {
        let mut acc = OtelMetricsAccumulator::default();

        let metrics = vec![make_cumulative_sum_metric(
            "test",
            make_number_time_series(vec![], vec![(30_000_000_000, 7), (45_000_000_000, 8)]),
        )];

        acc.accumulate(metrics);
        let metrics = acc.export();
        let data_points = assert_sum(assert_one_metric(metrics));
        assert_eq!(data_points.len(), 3);
        assert_eq!(data_points[0].time_unix_nano, 0);
        assert_eq!(assert_int(&data_points[0]), 0);
        assert_eq!(data_points[1].time_unix_nano, 30_000_000_000);
        assert_eq!(assert_int(&data_points[1]), 7);
        assert_eq!(data_points[2].time_unix_nano, 45_000_000_000);
        assert_eq!(assert_int(&data_points[2]), 8);
    }

    #[test]
    fn distant_point_is_preceded_by_extra_copy_of_the_last_point_before() {
        let mut acc = OtelMetricsAccumulator::default();

        let metrics = vec![make_cumulative_sum_metric(
            "test",
            make_number_time_series(vec![], vec![(30_000_000_000, 7), (120_000_000_000, 8)]),
        )];

        acc.accumulate(metrics);
        let metrics = acc.export();
        let data_points = assert_sum(assert_one_metric(metrics));
        assert_eq!(data_points.len(), 4);
        assert_eq!(data_points[0].time_unix_nano, 0);
        assert_eq!(assert_int(&data_points[0]), 0);
        assert_eq!(data_points[1].time_unix_nano, 30_000_000_000);
        assert_eq!(assert_int(&data_points[1]), 7);
        // This is the extra data point
        assert_eq!(data_points[2].time_unix_nano, 90_000_000_000);
        assert_eq!(assert_int(&data_points[2]), 7);
        assert_eq!(data_points[3].time_unix_nano, 120_000_000_000);
        assert_eq!(assert_int(&data_points[3]), 8);
    }

    #[test]
    fn second_export_emits_no_data_points() {
        let mut acc = OtelMetricsAccumulator::default();

        let metrics = vec![make_cumulative_sum_metric(
            "test",
            make_number_time_series(vec![], vec![(30_000_000_000, 7)]),
        )];

        acc.accumulate(metrics);
        acc.export();
        let second_export = acc.export();

        let data_points = assert_sum(assert_one_metric(second_export));
        assert!(data_points.is_empty());
    }

    #[test]
    fn different_time_series_and_metrics_are_not_mixed_up() {
        let mut acc = OtelMetricsAccumulator::default();

        let mut ts1_1 = make_number_time_series(vec![("attr1", "v1")], vec![(30_000_000_000, 7)]);
        let mut ts1_2 = make_number_time_series(vec![("attr1", "v2")], vec![(30_000_000_000, 8)]);
        let mut ts_1 = Vec::new();
        ts_1.append(&mut ts1_1);
        ts_1.append(&mut ts1_2);
        let metric1 = vec![make_cumulative_sum_metric("metric1", ts_1)];
        acc.accumulate(metric1);

        let ts2_1 = make_number_time_series(vec![("attr1", "v1")], vec![(30_000_000_000, 9)]);
        let metric2 = vec![make_cumulative_sum_metric("metric2", ts2_1)];
        acc.accumulate(metric2);

        let metrics = acc.export();
        assert_eq!(metrics.len(), 2);

        let metric1 = metrics
            .iter()
            .find(|m| m.name == "metric1")
            .unwrap()
            .clone();
        let data_points = assert_sum(metric1);
        assert_eq!(data_points.len(), 4);
        let expected_dp_1 = make_number_point(vec![("attr1", "v1")], 0, 0);
        let expected_dp_2 = make_number_point(vec![("attr1", "v1")], 30_000_000_000, 7);
        let expected_dp_3 = make_number_point(vec![("attr1", "v2")], 0, 0);
        let expected_dp_4 = make_number_point(vec![("attr1", "v2")], 30_000_000_000, 8);
        assert!(data_points.contains(&expected_dp_1));
        assert!(data_points.contains(&expected_dp_2));
        assert!(data_points.contains(&expected_dp_3));
        assert!(data_points.contains(&expected_dp_4));

        let metric2 = metrics
            .iter()
            .find(|m| m.name == "metric2")
            .unwrap()
            .clone();
        let data_points = assert_sum(metric2);
        assert_eq!(data_points.len(), 2);
        assert_eq!(data_points[0].time_unix_nano, 0);
        assert_eq!(assert_int(&data_points[0]), 0);
        assert_eq!(data_points[1].time_unix_nano, 30_000_000_000);
        assert_eq!(assert_int(&data_points[1]), 9);
    }

    #[test]
    fn gauges_are_not_modified() {
        let mut acc = OtelMetricsAccumulator::default();

        let metrics = vec![Metric {
            name: "test".to_owned(),
            description: "".to_owned(),
            unit: "".to_owned(),
            data: Some(metric::Data::Gauge(otel::Gauge {
                data_points: make_number_time_series(
                    vec![],
                    vec![(0, 7), (15_000_000_000, 7), (120_000_000_000, 8)],
                ),
            })),
        }];

        acc.accumulate(metrics);
        let metrics = acc.export();
        let data_points = assert_gauge(assert_one_metric(metrics));
        assert_eq!(data_points.len(), 3);
        assert_eq!(data_points[0].time_unix_nano, 0);
        assert_eq!(assert_int(&data_points[0]), 7);
        assert_eq!(data_points[1].time_unix_nano, 15_000_000_000);
        assert_eq!(assert_int(&data_points[1]), 7);
        assert_eq!(data_points[2].time_unix_nano, 120_000_000_000);
        assert_eq!(assert_int(&data_points[2]), 8);
    }
}
