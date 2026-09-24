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

use super::function_tags_provider::DynFunctionTagsProvider;
use super::lambda_function_arn::LambdaFunctionArn;
use crate::config::app_config::FunctionContextProviderConfig;
use lambda_extension::{Error, InvokeEvent};
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::sync::Mutex;
use tracing::{error, info, trace};

#[derive(Debug, Clone)]
pub struct FunctionContext {
    // the arn is always without version
    pub arn: LambdaFunctionArn,
    // the arn is always with version (not aliased)
    pub version_arn: LambdaFunctionArn,
    pub application_name: String,
    pub subsystem_name: String,
    pub service_name: String,
    pub lambda_function_version: String,
    pub lambda_instance_coralogix_id: String,
    pub tags: Option<HashMap<String, String>>,
}

pub struct FunctionContextProvider {
    info: LambdaInstanceInfo,
    config: FunctionContextProviderConfig,

    function_tags_provider: Option<DynFunctionTagsProvider>,

    function_context_cache: Mutex<FunctionContextCache>,
}

impl FunctionContextProvider {
    pub fn new(
        info: LambdaInstanceInfo,
        config: FunctionContextProviderConfig,
        function_tags_provider: Option<DynFunctionTagsProvider>,
    ) -> FunctionContextProvider {
        FunctionContextProvider {
            info,
            config,
            function_tags_provider,
            function_context_cache: Mutex::new(FunctionContextCache {
                timestamp: Instant::now(),
                function_context: None,
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LambdaInstanceInfo {
    pub aws_region: String,
    pub lambda_function_name: String,
    pub lambda_function_version: String,
    pub lambda_instance_coralogix_id: String,
}

#[derive(Debug, Clone)]
struct FunctionContextCache {
    timestamp: Instant,
    function_context: Option<Arc<FunctionContext>>,
}

impl FunctionContextProvider {
    // FunctionContextProvider needs to receive invoke events in order to learn the ARN of the function which is not known at startup.
    // This is also a trigger to check if the cache needs to be refreshed (tags fetched again). This is convenient because:
    // 1. on_invoke_event happens at a beginning of an invocation, so the operation of fetching tags will happen in parallel to the function doing it's job
    // 2. freezing of lambda is prevented as long as on_invoke_event is running
    pub async fn on_invoke_event(&self, e: &InvokeEvent) -> Result<Arc<FunctionContext>, Error> {
        let mut cache = self.function_context_cache.lock().await;

        if let Some(fc) = cache
            .function_context
            .as_ref()
            .filter(|_| cache.timestamp.elapsed() <= self.config.tag_cache_validity)
        {
            Ok(fc.clone())
        } else {
            let function_context = Arc::new(self.obtain_function_context(e).await?);
            cache.timestamp = Instant::now();
            cache.function_context = Some(function_context.clone());
            Ok(function_context)
        }
    }

    pub async fn get_function_context_even_if_degraded(&self) -> Arc<FunctionContext> {
        self.function_context_cache
            .lock()
            .await
            .function_context
            .clone()
            .unwrap_or_else(|| Arc::new(self.make_degraded_function_context()))
    }

    async fn obtain_function_context(&self, e: &InvokeEvent) -> Result<FunctionContext, Error> {
        let arn = e
            .invoked_function_arn
            .parse::<LambdaFunctionArn>()?
            .without_version();

        let tags = match &self.function_tags_provider {
            Some(function_tags_provider) => {
                trace!("Obtaining function tags");
                match function_tags_provider.obtain_function_tags(&arn).await {
                    Ok(tags) => {
                        info!("Obtained function tags.");
                        Some(tags)
                    }
                    Err(error) => {
                        error!(?error, "Failed to obtain tags.");
                        None
                    }
                }
            }
            None => None,
        };

        Ok(self.make_function_context(&arn, tags))
    }

    fn make_function_context(
        &self,
        arn: &LambdaFunctionArn,
        tags: Option<HashMap<String, String>>,
    ) -> FunctionContext {
        FunctionContext {
            arn: arn.clone(),
            version_arn: arn.clone().with_version(&self.info.lambda_function_version),
            application_name: self
                .config
                .configured_application
                .clone()
                .unwrap_or_else(|| arn.account_id.clone()), // TODO Consider using a different default that would be consistent with degraded mode. This would be a breaking change.
            subsystem_name: self
                .config
                .configured_subsystem
                .clone()
                .unwrap_or_else(|| self.info.lambda_function_name.clone()),
            service_name: self
                .config
                .configured_service_name
                .clone()
                .unwrap_or_else(|| self.info.lambda_function_name.clone()), // ADOT / opentelemetry-lambda also uses function name as service.name (https://github.com/open-telemetry/opentelemetry-lambda/blob/layer-javaagent/0.11.0/java/layer-wrapper/scripts/otel-handler#L9)
            lambda_function_version: self.info.lambda_function_version.clone(),
            lambda_instance_coralogix_id: self.info.lambda_instance_coralogix_id.clone(),
            tags,
        }
    }

    fn make_degraded_function_context(&self) -> FunctionContext {
        let version_arn = LambdaFunctionArn {
            partition: "unknown".to_owned(),
            region: self.info.aws_region.clone(),
            account_id: "unknown".to_owned(),
            function_name: self.info.lambda_function_name.clone(),
            version: Some(self.info.lambda_function_version.clone()),
        };
        FunctionContext {
            arn: version_arn.clone().without_version(),
            version_arn,
            application_name: self
                .config
                .configured_application
                .clone()
                .unwrap_or_else(|| "aws-lambda".to_owned()),
            subsystem_name: self
                .config
                .configured_subsystem
                .clone()
                .unwrap_or_else(|| self.info.lambda_function_name.clone()),
            service_name: self
                .config
                .configured_service_name
                .clone()
                .unwrap_or_else(|| self.info.lambda_function_name.clone()),
            lambda_function_version: self.info.lambda_function_version.clone(),
            lambda_instance_coralogix_id: self.info.lambda_instance_coralogix_id.clone(),
            tags: None,
        }
    }
}
