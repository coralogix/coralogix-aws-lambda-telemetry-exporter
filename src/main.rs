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

extern crate core;

use anyhow::anyhow;
use async_once_cell::Lazy;
use aws_config::{BehaviorVersion, SdkConfig};
use aws_smithy_http_client::{
    Builder as AwsHttpClientBuilder,
    tls::{self, rustls_provider::CryptoMode},
};
use coralogix_aws_lambda_telemetry_exporter::api_key::{ApiKey, get_api_key_from_secrets_manager};
use coralogix_aws_lambda_telemetry_exporter::built_info;
use coralogix_aws_lambda_telemetry_exporter::coralogix::combined_telemetry_sender::CombinedTelemetrySender;
use coralogix_aws_lambda_telemetry_exporter::coralogix::coralogix_sender::{
    CoralogixTelemetrySender, NoopEpsagonTracesTelemetrySender, OtlpPillarTelemetrySender,
};
use coralogix_aws_lambda_telemetry_exporter::coralogix::cx_otlp_sender::CxOtlpSender;
use coralogix_aws_lambda_telemetry_exporter::coralogix::{
    OtlpSenderParams, OtlpTarget, TelemetryExporterContext,
};
use coralogix_aws_lambda_telemetry_exporter::diagnostics::configure_tracing;
use coralogix_aws_lambda_telemetry_exporter::error_handling::ErrorHandlingMode;
use coralogix_aws_lambda_telemetry_exporter::firehose::firehose_sender::LiveFirehoseSender;
use coralogix_aws_lambda_telemetry_exporter::firehose::firehose_telemetry_sender::FirehoseTelemetrySender;
use coralogix_aws_lambda_telemetry_exporter::otlp_server::grpc::serve_otlp_grpc;
use coralogix_aws_lambda_telemetry_exporter::otlp_server::http::serve_otlp_http;
use coralogix_aws_lambda_telemetry_exporter::proto::opentelemetry::proto::logs::v1::ResourceLogs;
use coralogix_aws_lambda_telemetry_exporter::proto::opentelemetry::proto::metrics::v1::ResourceMetrics;
use coralogix_aws_lambda_telemetry_exporter::proto::opentelemetry::proto::trace::v1::ResourceSpans;
use coralogix_aws_lambda_telemetry_exporter::telemetry::function_context::{
    FunctionContextProvider, LambdaInstanceInfo,
};
use coralogix_aws_lambda_telemetry_exporter::telemetry::function_tags_provider::{
    AwsApiFunctionTagsProvider, DynFunctionTagsProvider,
};
use coralogix_aws_lambda_telemetry_exporter::telemetry::telemetry_sender::{
    CompositeTelemetrySender, DynBatchTelemetrySender, DynPillarTelemetrySender,
};
use rustls::crypto::CryptoProvider;
use std::env;
use std::iter::repeat_with;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

use coralogix_aws_lambda_telemetry_exporter::config::app_config::{
    AppConfig, KeySourceConfig, OtlpTargetConfig, TargetConfig,
};
use coralogix_aws_lambda_telemetry_exporter::coralogix::log_sender::OtlpLogSender;
use coralogix_aws_lambda_telemetry_exporter::coralogix::metric_sender::OtlpMetricSender;
use coralogix_aws_lambda_telemetry_exporter::coralogix::trace_sender::OtlpTraceSender;
use coralogix_aws_lambda_telemetry_exporter::lambda_event_receiver::LambdaEventReceiver;
use coralogix_aws_lambda_telemetry_exporter::telemetry::telemetry_service::TelemetryService;
use coralogix_aws_lambda_telemetry_exporter::telemetry_event_receiver::TelemetryEventReceiver;
use tokio::signal;
use tokio::signal::unix::{SignalKind, signal as unix_signal};

use LambdaEnvironmentError::*;
use lambda_extension::{Extension, LogBuffering, SharedService};
use tracing::{debug, info, trace, warn};

#[cfg(all(feature = "fips", feature = "non-fips"))]
compile_error!("features `fips` and `non-fips` are mutually exclusive");

#[cfg(not(any(feature = "fips", feature = "non-fips")))]
compile_error!("either `fips` or `non-fips` feature must be enabled");

const FIPS_ENABLED: bool = cfg!(feature = "fips");

fn fips_suffix() -> &'static str {
    if FIPS_ENABLED { " FIPS" } else { "" }
}

#[derive(thiserror::Error, Debug)]
pub enum LambdaEnvironmentError {
    #[error("Expected to find an environment variable {key} supplied by AWS Lambda")]
    VariableMissing { key: String },
}

fn get_required_env_var(key: &str) -> anyhow::Result<String> {
    env::var(key).map_err(|_| {
        anyhow!(VariableMissing {
            key: key.to_string(),
        })
    })
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    configure_tracing();

    let error_handling = ErrorHandlingMode::from_env();

    if let Err(e) = start_telemetry_exporter(error_handling).await {
        let msg = format!("Failed to start coralogix-aws-lambda-telemetry-exporter: {e}");
        error_handling.handle_error(&msg);
        start_dummy_telemetry_exporter(error_handling);
    }

    let mut sigterm = unix_signal(SignalKind::terminate())?;
    let mut sigint = unix_signal(SignalKind::interrupt())?;

    tokio::select! {
        _ = sigterm.recv() => {
            info!("SIGTERM signal has been received");
        }
        _ = sigint.recv() => {
            info!("SIGINT signal has been received");
        }
        _ = signal::ctrl_c() => {
            info!("User signal has been received");
        }
    }

    info!("Exiting");

    Ok(())
}

async fn start_telemetry_exporter(
    error_handling: ErrorHandlingMode,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Starting coralogix-aws-lambda-telemetry-exporter {}{}",
        built_info::PKG_VERSION,
        fips_suffix()
    );

    install_crypto_provider()?;

    let aws_region = get_required_env_var("AWS_REGION")?;
    let lambda_function_name = get_required_env_var("AWS_LAMBDA_FUNCTION_NAME")?;
    let lambda_function_version = get_required_env_var("AWS_LAMBDA_FUNCTION_VERSION")?;

    // OTEL recommends using log stream name as a unique ID of an instance of a Lambda function (https://github.com/open-telemetry/semantic-conventions/blob/v1.32.0/docs/resource/faas.md?plain=1#L71).
    // But lambda extensions don't have access to the AWS_LAMBDA_LOG_STREAM_NAME (https://docs.aws.amazon.com/lambda/latest/dg/runtimes-extensions-api.html)
    // Instead this code gives each instance a unique ID, which will be used as a label for the reported metrics.
    // Metrics have to have unique set of labels, otherwise metrics from two instances would overwrite each other
    let lambda_instance_coralogix_id = hex::encode(
        repeat_with(|| fastrand::u8(..))
            .take(16)
            .collect::<Vec<_>>(),
    );

    let config = Arc::new(AppConfig::load()?);

    debug!("Configuration: {:?}", config);

    let exporter_context = TelemetryExporterContext {
        telemetry_exporter_version: built_info::PKG_VERSION.to_string(),
    };

    let aws_config = Arc::pin(Lazy::new(async move {
        trace!("Loading AWS config.");
        let http_client = AwsHttpClientBuilder::new()
            .tls_provider(tls::Provider::Rustls(selected_aws_crypto_mode()))
            .build_https();

        aws_config::defaults(BehaviorVersion::v2026_01_12())
            .http_client(http_client)
            .use_fips(FIPS_ENABLED)
            .load()
            .await
    }));

    let function_tags_provider: Option<DynFunctionTagsProvider> = if config.tags_enabled {
        trace!("Setting up lambda client.");
        let lambda_client = aws_sdk_lambda::Client::new(&aws_config.as_ref().await.as_ref());
        Some(Arc::new(AwsApiFunctionTagsProvider { lambda_client }))
    } else {
        None
    };

    let params = OtlpSenderParams {
        context: exporter_context.clone(),
        // TODO make these configurable
        request_timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(3),
        alpn_enabled: config.target.alpn_enabled,
    };

    let mut api_key_from_secret: Option<ApiKey> = None;

    let main_sender: Option<DynBatchTelemetrySender> = match config.target.main.clone() {
        None => None,
        Some(TargetConfig::Firehose {
            delivery_stream_name,
        }) => {
            trace!("Setting up firehose client.");
            let firehose_client =
                aws_sdk_firehose::Client::new(&aws_config.as_ref().await.as_ref());

            let firehose_sender = Arc::new(LiveFirehoseSender::new(
                firehose_client,
                delivery_stream_name,
            ));

            Some(Arc::new(FirehoseTelemetrySender::new(firehose_sender)))
        }
        Some(TargetConfig::Otlp { target }) => {
            let target = make_target(target, &mut api_key_from_secret, &aws_config).await?;

            if config.target.combined_telemetry_enabled {
                let cx_otlp_sender = Arc::new(CxOtlpSender::new(&params, target)?);

                Some(Arc::new(CombinedTelemetrySender::new(cx_otlp_sender)))
            } else {
                let logs_telemetry_sender = Arc::new(OtlpPillarTelemetrySender::new(
                    "logs",
                    Arc::new(OtlpLogSender::new(&params, target.clone())?),
                ));
                let spans_telemetry_sender = Arc::new(OtlpPillarTelemetrySender::new(
                    "spans",
                    Arc::new(OtlpTraceSender::new(&params, target.clone())?),
                ));
                let metrics_telemetry_sender = Arc::new(OtlpPillarTelemetrySender::new(
                    "metrics",
                    Arc::new(OtlpMetricSender::new(&params, target)?),
                ));
                let epsagon_traces_telemetry_sender = Arc::new(NoopEpsagonTracesTelemetrySender {});

                Some(Arc::new(CoralogixTelemetrySender {
                    logs_telemetry_sender,
                    spans_telemetry_sender,
                    metrics_telemetry_sender,
                    epsagon_traces_telemetry_sender,
                }))
            }
        }
    };

    let logs_sender: Option<DynPillarTelemetrySender<ResourceLogs>> =
        match config.target.logs.clone() {
            None => None,
            Some(target) => {
                let target = make_target(target, &mut api_key_from_secret, &aws_config).await?;
                Some(Arc::new(OtlpPillarTelemetrySender::new(
                    "logs",
                    Arc::new(OtlpLogSender::new(&params, target)?),
                )))
            }
        };

    let traces_sender: Option<DynPillarTelemetrySender<ResourceSpans>> =
        match config.target.traces.clone() {
            None => None,
            Some(target) => {
                let target = make_target(target, &mut api_key_from_secret, &aws_config).await?;
                Some(Arc::new(OtlpPillarTelemetrySender::new(
                    "spans",
                    Arc::new(OtlpTraceSender::new(&params, target)?),
                )))
            }
        };

    let metrics_sender: Option<DynPillarTelemetrySender<ResourceMetrics>> =
        match config.target.metrics.clone() {
            None => None,
            Some(target) => {
                let target = make_target(target, &mut api_key_from_secret, &aws_config).await?;
                Some(Arc::new(OtlpPillarTelemetrySender::new(
                    "metrics",
                    Arc::new(OtlpMetricSender::new(&params, target)?),
                )))
            }
        };

    let composite_sender = Arc::new(CompositeTelemetrySender {
        logs: logs_sender,
        traces: traces_sender,
        metrics: metrics_sender,
        main: main_sender,
    });

    trace!("Setting up telemetry service.");

    let function_context_provider = FunctionContextProvider::new(
        LambdaInstanceInfo {
            aws_region,
            lambda_function_name,
            lambda_function_version,
            lambda_instance_coralogix_id,
        },
        config.function_context_provider_config.clone(),
        function_tags_provider,
    );

    let telemetry_service = Arc::new(TelemetryService::new(
        Arc::new(config.telemetry_service_config.clone()),
        function_context_provider,
        composite_sender,
    ));

    let otlp_server_enabled = config.otlp_server_enabled;
    let otlp_grpc_listener = if otlp_server_enabled {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 4317);
        let listener = TcpListener::bind(addr).await?;
        info!("OTLP gRPC server is listening on {}", addr);
        Some(listener)
    } else {
        None
    };

    let otlp_http_listener = if otlp_server_enabled {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 4318);
        let listener = TcpListener::bind(addr).await?;
        info!("OTLP HTTP server is listening on {}", addr);
        Some(listener)
    } else {
        None
    };

    let lambda_event_receiver = LambdaEventReceiver::new(telemetry_service.clone(), error_handling);
    let telemetry_event_receiver =
        TelemetryEventReceiver::new(telemetry_service.clone(), error_handling);

    let lambda_extension = Extension::new()
        .with_events_processor(lambda_event_receiver)
        .with_telemetry_buffering(LogBuffering {
            timeout_ms: config.aws_telemetry_interval_ms,
            max_bytes: 262144,
            max_items: 1000,
        })
        .with_telemetry_port_number(27549)
        .with_telemetry_processor(SharedService::new(telemetry_event_receiver));

    if let Some(listener) = otlp_grpc_listener {
        let telemetry_service = telemetry_service.clone();
        tokio::spawn(async move {
            match serve_otlp_grpc(listener, telemetry_service).await {
                Ok(_) => error_handling.handle_error("OTLP gRPC server stopped."),
                Err(error) => {
                    error_handling.handle_error(&format!("OTLP gRPC server crashed: {error}"))
                }
            }
        });
    }

    if let Some(listener) = otlp_http_listener {
        let telemetry_service = telemetry_service.clone();
        tokio::spawn(async move {
            match serve_otlp_http(listener, telemetry_service).await {
                Ok(_) => error_handling.handle_error("OTLP HTTP server stopped."),
                Err(error) => {
                    error_handling.handle_error(&format!("OTLP HTTP server crashed: {error}"))
                }
            }
        });
    }

    tokio::spawn(async move {
        match lambda_extension.run().await {
            Ok(_) => error_handling.handle_error("Extension stopped."),
            Err(error) => error_handling.handle_error(&format!("Extension crashed: {error}")),
        }
    });

    info!("Started coralogix-aws-lambda-telemetry-exporter");

    Ok(())
}

#[cfg(feature = "fips")]
fn install_crypto_provider() -> Result<(), Box<dyn std::error::Error>> {
    match CryptoProvider::get_default() {
        Some(provider) if provider.fips() => Ok(()),
        Some(_) => Err(anyhow!("rustls default crypto provider is not FIPS-enabled").into()),
        None => {
            let provider = rustls::crypto::default_fips_provider();
            if !provider.fips() {
                return Err(anyhow!("rustls FIPS provider is not FIPS-enabled").into());
            }
            CryptoProvider::install_default(provider)
                .map_err(|_| anyhow!("failed to install rustls crypto provider"))?;
            Ok(())
        }
    }
}

#[cfg(feature = "non-fips")]
fn install_crypto_provider() -> Result<(), Box<dyn std::error::Error>> {
    match CryptoProvider::get_default() {
        Some(_) => Ok(()),
        None => {
            let provider = rustls::crypto::aws_lc_rs::default_provider();
            CryptoProvider::install_default(provider)
                .map_err(|_| anyhow!("failed to install rustls crypto provider"))?;
            Ok(())
        }
    }
}

#[cfg(feature = "fips")]
fn selected_aws_crypto_mode() -> CryptoMode {
    CryptoMode::AwsLcFips
}

#[cfg(feature = "non-fips")]
fn selected_aws_crypto_mode() -> CryptoMode {
    CryptoMode::AwsLc
}

async fn make_target(
    target: OtlpTargetConfig,
    api_key_from_secret: &mut Option<ApiKey>,
    aws_config: &Pin<Arc<Lazy<SdkConfig, impl Future<Output = SdkConfig>>>>,
) -> Result<OtlpTarget, Box<dyn std::error::Error>> {
    match target {
        OtlpTargetConfig::Coralogix {
            domain,
            key_source_config,
        } => {
            let api_key = match key_source_config {
                KeySourceConfig::EnvVar { key } => key,
                KeySourceConfig::SecretsManager { secret_id } => {
                    if let Some(api_key) = api_key_from_secret.clone() {
                        api_key
                    } else {
                        let aws_config = aws_config.as_ref().await;
                        trace!("Loading API KEY from SecretsManager.");
                        let api_key =
                            get_api_key_from_secrets_manager(&aws_config, secret_id).await?;
                        *api_key_from_secret = Some(api_key.clone());
                        api_key
                    }
                }
            };
            Ok(OtlpTarget::Coralogix { domain, api_key })
        }
        OtlpTargetConfig::Otel { url } => Ok(OtlpTarget::Otel { url }),
    }
}

fn start_dummy_telemetry_exporter(error_handling: ErrorHandlingMode) {
    warn!(
        "Starting dummy coralogix-aws-lambda-telemetry-exporter to avoid crashing the lambda function"
    );

    let lambda_extension = Extension::new();

    tokio::spawn(async move {
        match lambda_extension.run().await {
            Ok(_) => error_handling.handle_error("Extension stopped."),
            Err(error) => error_handling.handle_error(&format!("Extension crashed: {error}")),
        }
    });

    warn!("Started dummy coralogix-aws-lambda-telemetry-exporter");
}
