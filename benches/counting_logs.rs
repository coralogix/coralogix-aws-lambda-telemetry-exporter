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

use std::iter::repeat_with;

use coralogix_aws_lambda_telemetry_exporter::proto::opentelemetry::proto::logs::v1::{
    LogRecord, ResourceLogs, ScopeLogs,
};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref INPUT: Vec<ResourceLogs> = vec![ResourceLogs {
        scope_logs: repeat_with(scope_logs).take(50).collect(),
        ..ResourceLogs::default()
    }];
}

fn scope_logs() -> ScopeLogs {
    ScopeLogs {
        log_records: repeat_with(LogRecord::default).take(100).collect(),
        ..ScopeLogs::default()
    }
}

fn fold_based_implementation(logs: &[ResourceLogs]) -> usize {
    logs.iter()
        .flat_map(|rl| &rl.scope_logs)
        .fold(0, |acc, sl| acc + sl.log_records.len())
}

fn sum_based_implementation(logs: &[ResourceLogs]) -> usize {
    logs.iter()
        .flat_map(|rl| &rl.scope_logs)
        .map(|sl| sl.log_records.len())
        .sum()
}

fn count_logs(c: &mut Criterion) {
    c.bench_function("fold_based_implementation", move |b| {
        b.iter_batched(|| &INPUT, fold_based_implementation, BatchSize::SmallInput)
    });

    c.bench_function("sum_based_implementation", move |b| {
        b.iter_batched(|| &INPUT, sum_based_implementation, BatchSize::SmallInput)
    });
}

criterion_group!(benches, count_logs);
criterion_main!(benches);
