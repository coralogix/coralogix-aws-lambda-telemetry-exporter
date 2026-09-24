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

use self::app_config::AppConfigError;
use std::env;
use std::env::VarError;
use std::str::FromStr;
use std::time::Duration;

pub mod app_config;
#[cfg(test)]
mod tests;

fn get_parsable<T>(key: &str, default: T) -> Result<T, T::Err>
where
    T: FromStr,
{
    get_string_option(key).map_or_else(|| Ok(default), |s| s.parse::<T>())
}

fn get_string(key: &str) -> Result<String, AppConfigError> {
    get_env_var_with_prefix(key)
        .map(|s| s.trim().to_owned())
        .map_err(|_| AppConfigError::VariableMissing {
            key: key.to_owned(),
        })
}

fn get_string_option(key: &str) -> Option<String> {
    get_env_var_with_prefix(key)
        .map(|s| s.trim().to_owned())
        .ok()
}

fn get_bool_option(key: &str) -> Result<Option<bool>, AppConfigError> {
    match get_env_var_with_prefix(key) {
        Ok(s) => match s.trim().to_lowercase().as_str() {
            "true" | "t" => Ok(Some(true)),
            "false" | "f" => Ok(Some(false)),
            _ => Err(AppConfigError::InvalidBoolean {
                key: key.to_owned(),
                value: s.to_owned(),
            }),
        },
        Err(_) => Ok(None),
    }
}

fn get_i64_option(key: &str) -> Result<Option<i64>, AppConfigError> {
    match get_env_var_with_prefix(key) {
        Ok(s) => match s.trim().parse::<i64>() {
            Ok(i) => Ok(Some(i)),
            Err(_) => Err(AppConfigError::InvalidInteger {
                key: key.to_owned(),
                value: s.to_owned(),
            }),
        },
        Err(_) => Ok(None),
    }
}

fn get_duration_ms(key: &str, default: u64) -> Result<Duration, AppConfigError> {
    Ok(get_u64_option(key)?.map_or_else(|| Duration::from_millis(default), Duration::from_millis))
}

fn get_u64_option(key: &str) -> Result<Option<u64>, AppConfigError> {
    match get_env_var_with_prefix(key) {
        Ok(s) => match s.trim().parse::<u64>() {
            Ok(i) => Ok(Some(i)),
            Err(_) => Err(AppConfigError::InvalidInteger {
                key: key.to_owned(),
                value: s.to_owned(),
            }),
        },
        Err(_) => Ok(None),
    }
}

fn get_usize_option(key: &str) -> Result<Option<usize>, AppConfigError> {
    match get_env_var_with_prefix(key) {
        Ok(s) => match s.trim().parse::<usize>() {
            Ok(i) => Ok(Some(i)),
            Err(_) => Err(AppConfigError::InvalidInteger {
                key: key.to_owned(),
                value: s.to_owned(),
            }),
        },
        Err(_) => Ok(None),
    }
}

fn get_env_var_with_prefix(key: &str) -> Result<String, VarError> {
    match env::var(format!("CORALOGIX_{key}")) {
        // CORALOGIX_ is supported for backward compatibility
        Ok(s) => Ok(s),
        Err(_) => env::var(format!("CX_{key}")), // CX_ prefix is supported as a way to reduce the size of the config and is currently the preferred prefix
    }
}
