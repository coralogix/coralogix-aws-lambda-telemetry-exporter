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

use crate::proto::opentelemetry::proto::common::v1::any_value::Value;
use crate::proto::opentelemetry::proto::common::v1::{
    AnyValue, ArrayValue, KeyValue, KeyValueList,
};

impl From<serde_json::Value> for AnyValue {
    fn from(json: serde_json::Value) -> Self {
        let value = match json {
            serde_json::Value::Null => None,
            serde_json::Value::Bool(b) => Some(Value::BoolValue(b)),
            serde_json::Value::Number(n) => Some(if let Some(i) = n.as_i64() {
                Value::IntValue(i)
            } else if let Some(f) = n.as_f64() {
                Value::DoubleValue(f)
            } else {
                Value::StringValue(n.to_string())
            }),
            serde_json::Value::String(s) => Some(Value::StringValue(s)),
            serde_json::Value::Array(a) => Some(Value::ArrayValue(ArrayValue {
                values: a.into_iter().map(AnyValue::from).collect::<Vec<AnyValue>>(),
            })),
            serde_json::Value::Object(o) => Some(Value::KvlistValue(KeyValueList {
                values: o
                    .into_iter()
                    .map(|(k, v)| KeyValue {
                        key: k,
                        value: Some(AnyValue::from(v)),
                    })
                    .collect::<Vec<KeyValue>>(),
            })),
        };
        AnyValue { value }
    }
}
