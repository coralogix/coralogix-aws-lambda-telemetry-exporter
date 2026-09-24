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

use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::codegen::CompressionEncoding;
use tonic::{Request, Response, Status, transport::Server};
use tracing::{debug, error};

use crate::proto::opentelemetry::proto::collector::metrics::v1::metrics_service_server::{
    MetricsService, MetricsServiceServer,
};
use crate::proto::opentelemetry::proto::collector::metrics::v1::{
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use crate::proto::opentelemetry::proto::collector::trace::v1::trace_service_server::{
    TraceService, TraceServiceServer,
};
use crate::proto::opentelemetry::proto::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use crate::telemetry::telemetry_sender::ItemCount;
use crate::telemetry::telemetry_service::TelemetryService;

pub async fn serve_otlp_grpc(
    listener: TcpListener,
    telemetry_service: Arc<TelemetryService>,
) -> Result<(), tonic::transport::Error> {
    let trace_service = TraceServiceServer::new(OtlpGrpcTraceReceiver {
        telemetry_service: telemetry_service.clone(),
    })
    .send_compressed(CompressionEncoding::Gzip)
    .accept_compressed(CompressionEncoding::Gzip);

    let metrics_service = MetricsServiceServer::new(OtlpGrpcMetricsReceiver {
        telemetry_service: telemetry_service.clone(),
    })
    .send_compressed(CompressionEncoding::Gzip)
    .accept_compressed(CompressionEncoding::Gzip);

    let builder = Server::builder()
        .timeout(Duration::from_secs(10))
        .add_service(trace_service)
        .add_service(metrics_service);

    builder
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
}

struct OtlpGrpcTraceReceiver {
    telemetry_service: Arc<TelemetryService>,
}

#[tonic::async_trait]
impl TraceService for OtlpGrpcTraceReceiver {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        debug!(
            "Received {} function spans via gRPC",
            request.get_ref().resource_spans.item_count()
        );
        let result = self
            .telemetry_service
            .handle_otlp_spans(request.into_inner())
            .await;

        match result {
            Ok(_) => Ok(Response::new(ExportTraceServiceResponse {
                partial_success: None,
            })),
            Err(error) => {
                error!(?error, "Internal error");
                Err(Status::internal(error.to_string()))
            }
        }
    }
}

struct OtlpGrpcMetricsReceiver {
    telemetry_service: Arc<TelemetryService>,
}

#[tonic::async_trait]
impl MetricsService for OtlpGrpcMetricsReceiver {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        debug!(
            "Received {} function metrics via gRPC",
            request.get_ref().resource_metrics.item_count()
        );
        let result = self
            .telemetry_service
            .handle_otlp_metrics(request.into_inner())
            .await;

        match result {
            Ok(()) => Ok(Response::new(ExportMetricsServiceResponse {
                partial_success: None,
            })),
            Err(error) => {
                error!(?error, "Internal error");
                Err(Status::internal(error.to_string()))
            }
        }
    }
}
