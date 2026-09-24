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

use super::Error;
use crate::proto::com::coralogix::ingress::otlp::v1::Telemetry;
use anyhow::anyhow;
use async_trait::async_trait;
use aws_sdk_firehose::primitives::Blob;
use aws_sdk_firehose::types::Record;
use flate2::Compression;
use flate2::write::GzEncoder;
use prost::Message;
use std::io::Write;
use std::sync::Arc;

pub type DynFirehoseSender = Arc<dyn FirehoseSender + Send + Sync>;

#[async_trait]
pub trait FirehoseSender {
    async fn send(&self, telemetry: Telemetry) -> Result<(), Error>;
}

#[derive(Clone)]
pub struct LiveFirehoseSender {
    firehose_client: aws_sdk_firehose::Client,
    delivery_stream_name: String,
}

impl LiveFirehoseSender {
    pub fn new(firehose_client: aws_sdk_firehose::Client, delivery_stream_name: String) -> Self {
        LiveFirehoseSender {
            firehose_client,
            delivery_stream_name,
        }
    }
}

#[async_trait]
impl FirehoseSender for LiveFirehoseSender {
    async fn send(&self, telemetry: Telemetry) -> Result<(), Error> {
        let record = into_firehose_record(telemetry).map_err(|e| Error::Internal(anyhow!(e)))?;

        let result = self
            .firehose_client
            .put_record()
            .delivery_stream_name(&self.delivery_stream_name)
            .set_record(Some(record))
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(Error::Communication(e)),
        }
    }
}

fn into_firehose_record(telemetry: Telemetry) -> Result<Record, crate::Error> {
    let encoded = telemetry.encode_to_vec();
    let compressed = gzip(encoded)?;
    Ok(Record::builder().data(Blob::new(compressed)).build()?)
}

fn gzip(data: Vec<u8>) -> std::io::Result<Vec<u8>> {
    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(&data)?;
    gzip.finish()
}
