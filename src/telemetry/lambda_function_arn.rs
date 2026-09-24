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

use std::{fmt::Display, str::FromStr};

use thiserror::Error;

#[derive(Clone, Debug)]
pub struct LambdaFunctionArn {
    pub partition: String,
    pub region: String,
    pub account_id: String,
    pub function_name: String,
    pub version: Option<String>,
}

impl LambdaFunctionArn {
    pub fn without_version(self) -> LambdaFunctionArn {
        LambdaFunctionArn {
            version: None,
            ..self
        }
    }

    pub fn with_version(self, version: &str) -> LambdaFunctionArn {
        LambdaFunctionArn {
            version: Some(version.to_owned()),
            ..self
        }
    }
}

impl FromStr for LambdaFunctionArn {
    type Err = LambdaFunctionArnParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let segments = s.split(':').collect::<Vec<_>>();
        match segments.as_slice() {
            [
                "arn",
                partition,
                "lambda",
                region,
                account_id,
                "function",
                function_name,
            ] => Ok(LambdaFunctionArn {
                partition: partition.to_string(),
                region: region.to_string(),
                account_id: account_id.to_string(),
                function_name: function_name.to_string(),
                version: None,
            }),
            [
                "arn",
                partition,
                "lambda",
                region,
                account_id,
                "function",
                function_name,
                version,
            ] => Ok(LambdaFunctionArn {
                partition: partition.to_string(),
                region: region.to_string(),
                account_id: account_id.to_string(),
                function_name: function_name.to_string(),
                version: Some(version.to_string()),
            }),
            _ => Err(LambdaFunctionArnParseError::InvalidLambdaFunctionArn(
                s.to_owned(),
            )),
        }
    }
}

#[derive(Error, Debug)]
pub enum LambdaFunctionArnParseError {
    #[error("'{0}' is not a valid AWS Lambda function ARN")]
    InvalidLambdaFunctionArn(String),
}

impl Display for LambdaFunctionArn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.version {
            Some(version) => write!(
                f,
                "arn:{}:lambda:{}:{}:function:{}:{}",
                self.partition, self.region, self.account_id, self.function_name, version
            ),
            None => write!(
                f,
                "arn:{}:lambda:{}:{}:function:{}",
                self.partition, self.region, self.account_id, self.function_name
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LambdaFunctionArn;

    #[test]
    fn parse_without_version() {
        let arn = "arn:aws:lambda:eu-west-1:200000000000:function:lambda-telemetry-test"
            .parse::<LambdaFunctionArn>()
            .expect("should parse");

        assert_eq!(arn.region, "eu-west-1");
        assert_eq!(arn.account_id, "200000000000");
        assert_eq!(arn.function_name, "lambda-telemetry-test");
        assert_eq!(arn.version, None);
    }

    #[test]
    fn parse_with_version() {
        let arn = "arn:aws:lambda:eu-west-1:200000000000:function:lambda-telemetry-test:$LATEST"
            .parse::<LambdaFunctionArn>()
            .expect("should parse");

        assert_eq!(arn.region, "eu-west-1");
        assert_eq!(arn.account_id, "200000000000");
        assert_eq!(arn.function_name, "lambda-telemetry-test");
        assert_eq!(arn.version, Some("$LATEST".to_owned()));
    }

    #[test]
    fn parse_and_then_to_string_is_identity_without_version() {
        let arn_string = "arn:aws:lambda:eu-west-1:200000000000:function:lambda-telemetry-test";
        assert_eq!(
            arn_string.parse::<LambdaFunctionArn>().unwrap().to_string(),
            arn_string
        );
    }

    #[test]
    fn parse_and_then_to_string_is_identity_with_version() {
        let arn_string =
            "arn:aws:lambda:eu-west-1:200000000000:function:lambda-telemetry-test:$LATEST";
        assert_eq!(
            arn_string.parse::<LambdaFunctionArn>().unwrap().to_string(),
            arn_string
        );
    }

    #[test]
    fn parse_and_then_to_string_is_identity_for_gov_cloud() {
        let arn_string = "arn:aws-us-gov:lambda:us-gov-east-1:200000000000:function:lambda-telemetry-test:test-alias";
        assert_eq!(
            arn_string.parse::<LambdaFunctionArn>().unwrap().to_string(),
            arn_string
        );
    }
}
