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

use std::fmt::Debug;

use aws_config::SdkConfig;
use aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError;
use derive_more::From;
use serde::Serialize;

#[derive(PartialEq, Eq, Clone, From, Serialize)]
pub struct ApiKey(String);

impl Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(***)")
    }
}

impl From<&str> for ApiKey {
    fn from(value: &str) -> Self {
        ApiKey(value.to_owned())
    }
}

impl ApiKey {
    pub fn token(&self) -> &str {
        &self.0
    }
}

#[derive(thiserror::Error, Debug)]
pub enum KeySourceError {
    #[error(
        "Failed to access AWS Secrets Manager. Please make sure the lambda function has permissions to access the {secret_id} secret. Error: {error:?}"
    )]
    FailedToAccessSecretsManager {
        secret_id: String,
        error: GetSecretValueError,
    },
    #[error("Didn't find the {secret_id} secret in AWS secretsmanager")]
    MissingSecret { secret_id: String },
}

pub async fn get_api_key_from_secrets_manager(
    aws_config: &SdkConfig,
    secret_id: String,
) -> Result<ApiKey, Box<dyn std::error::Error>> {
    let secretsmanager = aws_sdk_secretsmanager::Client::new(aws_config);
    let response = secretsmanager
        .get_secret_value()
        .set_secret_id(Some(secret_id.clone()))
        .send()
        .await
        .map_err(|error| KeySourceError::FailedToAccessSecretsManager {
            secret_id: secret_id.clone(),
            error: error.into_service_error(),
        })?;
    let secret = response
        .secret_string
        .ok_or(KeySourceError::MissingSecret { secret_id })?;
    Ok(ApiKey::from(secret))
}
