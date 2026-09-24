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

use async_trait::async_trait;
use tonic::Request;
use tonic::codegen::CompressionEncoding;
use tonic::transport::Endpoint;

use crate::proto::opentelemetry::proto::collector::metrics::v1::ExportMetricsServiceRequest;
use crate::proto::opentelemetry::proto::collector::metrics::v1::metrics_service_client::MetricsServiceClient;
use crate::proto::opentelemetry::proto::metrics::v1::ResourceMetrics;

use super::OtlpExportResponse;
use super::OtlpSender;
use super::OtlpSenderParams;
use super::OtlpTarget;
use super::{Error, add_authorization_metadata, handle_service_error, make_endpoint};

#[derive(Clone)]
pub struct OtlpMetricSender {
    target: OtlpTarget,
    endpoint: Endpoint,
}

impl OtlpMetricSender {
    pub fn new(params: &OtlpSenderParams, target: OtlpTarget) -> anyhow::Result<Self> {
        let endpoint = make_endpoint(params, &target)?;
        Ok(OtlpMetricSender { target, endpoint })
    }
}

#[async_trait]
impl OtlpSender<Vec<ResourceMetrics>> for OtlpMetricSender {
    async fn send(
        &self,
        resource_metrics: Vec<ResourceMetrics>,
    ) -> Result<OtlpExportResponse, Error> {
        let mut tonic_request = Request::new(ExportMetricsServiceRequest { resource_metrics });
        add_authorization_metadata(&mut tonic_request, &self.target)?;

        // For now we don't reuse connections between calls to the service and that is intentional and preferred in the context of lambda (because freezing may break connection in the pool).
        // TODO consider reuse of connection to save on TLS handshakes (with extensive testing of impact of freezing)
        let channel = self.endpoint.connect_lazy();

        let mut client = MetricsServiceClient::new(channel)
            .accept_compressed(CompressionEncoding::Gzip)
            .send_compressed(CompressionEncoding::Gzip);

        let response = client
            .export(tonic_request)
            .await
            .map_err(|e| handle_service_error(self.endpoint.uri(), e))?;

        match response.into_inner().partial_success {
            // Handling follows the otel documentation: https://github.com/open-telemetry/opentelemetry-proto/blob/v1.5.0/opentelemetry/proto/collector/trace/v1/trace_service.proto#L46-L60
            None => Ok(OtlpExportResponse::Success),
            Some(partial_success)
                if partial_success.rejected_data_points == 0
                    && partial_success.error_message.is_empty() =>
            {
                Ok(OtlpExportResponse::Success)
            }
            Some(partial_success) if partial_success.rejected_data_points == 0 => {
                Ok(OtlpExportResponse::Warning {
                    message: partial_success.error_message,
                })
            }
            Some(partial_success) => Ok(OtlpExportResponse::PartialSuccess {
                message: partial_success.error_message,
                dropped_items: partial_success.rejected_data_points,
            }),
        }
    }
}
