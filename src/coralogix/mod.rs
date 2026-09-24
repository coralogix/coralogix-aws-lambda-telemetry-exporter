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

use anyhow::Context;
use async_trait::async_trait;
use hyper::Uri;
use thiserror::Error;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::{Code, Request};

use crate::api_key::ApiKey;

pub mod combined_telemetry_sender;
pub mod coralogix_sender;
pub mod cx_otlp_sender;
pub mod log_sender;
pub mod metric_sender;
pub mod trace_sender;

pub type DynOtlpSender<T> = Arc<dyn OtlpSender<T> + Send + Sync>;

#[async_trait]
pub trait OtlpSender<T> {
    async fn send(&self, t: T) -> Result<OtlpExportResponse, Error>;
}

#[derive(Clone, Debug)]
pub struct OtlpSenderParams {
    pub context: TelemetryExporterContext,
    pub request_timeout: Duration,
    pub connect_timeout: Duration,
    pub alpn_enabled: bool,
}

#[derive(Clone, Debug)]
pub struct TelemetryExporterContext {
    pub telemetry_exporter_version: String,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum OtlpTarget {
    Coralogix { domain: String, api_key: ApiKey },
    Otel { url: String },
}

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Unauthenticated(tonic::Status),
    #[error(transparent)]
    BadRequest(tonic::Status),
    #[error("Blocked due to exceeding quota: {0}")]
    QuotaExceeded(tonic::Status),
    #[error("Permission denied: {0}")]
    PermissionDenied(tonic::Status),
    #[error("Failed to communicate with Coralogix API {0} : {1}")]
    Unavailable(Uri, tonic::Status),
    #[error(transparent)]
    UnexpectedResponse(tonic::Status),
    #[error(transparent)]
    Internal(anyhow::Error),
}

impl Error {
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Unauthenticated(_) => false,
            Error::BadRequest(_) => false,
            Error::QuotaExceeded(_) => false,
            Error::PermissionDenied(_) => false,
            Error::Unavailable(_, _) => true,
            Error::UnexpectedResponse(_) => true,
            Error::Internal(_) => false,
        }
    }
}

pub enum OtlpExportResponse {
    Success,
    Warning { message: String },
    PartialSuccess { message: String, dropped_items: i64 },
}

fn make_endpoint(params: &OtlpSenderParams, target: &OtlpTarget) -> anyhow::Result<Endpoint> {
    let url = match target {
        OtlpTarget::Coralogix { domain, api_key: _ } => format!("https://ingress.{domain}:443"),
        OtlpTarget::Otel { url } => url.clone(),
    };

    let is_tls = url.starts_with("https://");

    let endpoint = Channel::from_shared(url)?
        .timeout(params.request_timeout)
        .connect_timeout(params.connect_timeout)
        .user_agent(format!(
            "coralogix-aws-lambda-telemetry-exporter/{}",
            &params.context.telemetry_exporter_version
        ))?;

    let endpoint = if is_tls {
        endpoint.tls_config(
            ClientTlsConfig::default()
                .with_enabled_roots()
                .assume_http2(!params.alpn_enabled),
        )?
    } else {
        endpoint
    };

    Ok(endpoint)
}

#[allow(clippy::result_large_err)]
fn add_authorization_metadata<T>(
    request: &mut Request<T>,
    target: &OtlpTarget,
) -> Result<(), Error> {
    if let OtlpTarget::Coralogix { domain: _, api_key } = target {
        request.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {}", api_key.token()))
                .context("Private key container characters not valid for ASCII gRPC metadata")
                .map_err(Error::Internal)?,
        );
    }
    Ok(())
}

fn handle_service_error(uri: &Uri, status: tonic::Status) -> Error {
    match status.code() {
        // These errors can be returned by the tracing-ingress itself
        Code::Unauthenticated => Error::Unauthenticated(status),
        Code::ResourceExhausted => Error::QuotaExceeded(status),
        Code::InvalidArgument => Error::BadRequest(status),
        Code::Internal => Error::Unavailable(uri.clone(), status),
        // These errors are from the transport layer
        Code::DeadlineExceeded => Error::Unavailable(uri.clone(), status),
        Code::Unavailable => Error::Unavailable(uri.clone(), status),
        // Blocking on istio
        Code::PermissionDenied => Error::PermissionDenied(status),
        // Unexpected errors
        _ => Error::UnexpectedResponse(status),
    }
}

fn sending_verb(is_retry: bool) -> &'static str {
    if is_retry { "Resending" } else { "Sending" }
}
