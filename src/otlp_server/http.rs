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

use crate::Error;
use crate::telemetry::telemetry_sender::ItemCount;
use axum::Router;
use axum::extract::State;
use axum::routing::post;
use hyper::StatusCode;
use tokio::net::TcpListener;
use tonic::Code;
use tonic_types::Status;
use tracing::{debug, error};

use super::proto::ProtoBuf;
use crate::proto::opentelemetry::proto::collector::metrics::v1::{
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use crate::proto::opentelemetry::proto::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use crate::telemetry::telemetry_service::TelemetryService;

pub async fn serve_otlp_http(
    listener: TcpListener,
    telemetry_service: Arc<TelemetryService>,
) -> Result<(), Error> {
    let router = Router::new()
        .route("/v1/traces", post(handle_traces_export))
        .route("/v1/metrics", post(handle_metrics_export))
        .with_state(telemetry_service);

    axum::serve(listener, router.into_make_service()).await?;

    Ok(())
}

async fn handle_traces_export(
    State(telemetry_service): State<Arc<TelemetryService>>,
    request: ProtoBuf<ExportTraceServiceRequest>,
) -> Result<ProtoBuf<ExportTraceServiceResponse>, (StatusCode, ProtoBuf<Status>)> {
    debug!(
        "Received {} function spans via HTTP",
        &request.message.resource_spans.item_count()
    );

    let result = telemetry_service.handle_otlp_spans(request.message).await;

    match result {
        Ok(_) => Ok(ProtoBuf::from(ExportTraceServiceResponse {
            partial_success: None,
        })),
        Err(error) => Err(to_error_response(error)),
    }
}

async fn handle_metrics_export(
    State(telemetry_service): State<Arc<TelemetryService>>,
    request: ProtoBuf<ExportMetricsServiceRequest>,
) -> Result<ProtoBuf<ExportMetricsServiceResponse>, (StatusCode, ProtoBuf<Status>)> {
    debug!(
        "Received {} function metrics via HTTP",
        &request.message.resource_metrics.item_count()
    );
    let result = telemetry_service.handle_otlp_metrics(request.message).await;

    match result {
        Ok(_) => Ok(ProtoBuf::from(ExportMetricsServiceResponse {
            partial_success: None,
        })),
        Err(error) => Err(to_error_response(error)),
    }
}

fn to_error_response(error: Error) -> (StatusCode, ProtoBuf<Status>) {
    error!(?error, "Internal error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        ProtoBuf::from(Status {
            code: Code::Internal.into(),
            message: error.to_string(),
            details: Vec::new(),
        }),
    )
}
