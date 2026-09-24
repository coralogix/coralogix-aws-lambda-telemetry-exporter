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

#![warn(clippy::str_to_string)]
#![allow(unused_doc_comments)]
extern crate tracing;

pub mod api_key;
pub mod config;
pub mod coralogix;
pub mod diagnostics;
pub mod error_handling;
pub mod firehose;
pub mod lambda_event_receiver;
pub mod otlp_server;
pub mod proto;
pub mod telemetry;
pub mod telemetry_event_receiver;

pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;
