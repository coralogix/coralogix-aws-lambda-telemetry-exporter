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

use std::collections::HashMap;
use std::sync::Arc;

use crate::Error;
use async_trait::async_trait;
use aws_sdk_lambda::operation::get_function::GetFunctionOutput;

use super::lambda_function_arn::LambdaFunctionArn;

pub type DynFunctionTagsProvider = Arc<dyn FunctionTagsProvider + Send + Sync>;

#[async_trait]
pub trait FunctionTagsProvider {
    async fn obtain_function_tags(
        &self,
        arn: &LambdaFunctionArn, // TODO current implementation uses only function_name, this fact may be used to enable collecting tags during initialisation when ARN is not known, but it's unclear if that's a good idea.
    ) -> Result<HashMap<String, String>, Error>;
}

pub struct AwsApiFunctionTagsProvider {
    pub lambda_client: aws_sdk_lambda::Client,
}

#[async_trait]
impl FunctionTagsProvider for AwsApiFunctionTagsProvider {
    async fn obtain_function_tags(
        &self,
        arn: &LambdaFunctionArn,
    ) -> Result<HashMap<String, String>, Error> {
        let output: GetFunctionOutput = self
            .lambda_client
            .get_function()
            .function_name(arn.function_name.clone())
            .send()
            .await?;

        Ok(output.tags().cloned().unwrap_or_default())
    }
}
