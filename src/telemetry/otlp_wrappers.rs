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

use super::function_context::FunctionContext;
use super::string_attribute;
use crate::config::app_config::AttributeConfig;
use crate::proto::opentelemetry::proto::common::v1::InstrumentationScope;
use crate::proto::opentelemetry::proto::logs::v1::{LogRecord, ResourceLogs, ScopeLogs};
use crate::proto::opentelemetry::proto::metrics::v1 as otel_metrics;
use crate::proto::opentelemetry::proto::metrics::v1::{ResourceMetrics, ScopeMetrics};
use crate::proto::opentelemetry::proto::resource::v1::Resource;
use crate::proto::opentelemetry::proto::trace::v1 as otel_trace;
use crate::proto::opentelemetry::proto::trace::v1::{ResourceSpans, ScopeSpans};

pub fn wrap_logs_in_scope(
    log_records: Vec<LogRecord>,
    function_context: &FunctionContext,
) -> ScopeLogs {
    ScopeLogs {
        scope: Some(instrumentation_scope(function_context)),
        log_records,
        schema_url: "".to_owned(),
    }
}

pub fn wrap_logs_in_resource(
    scope_logs: Vec<ScopeLogs>,
    function_context: &FunctionContext,
    config: &AttributeConfig,
) -> ResourceLogs {
    ResourceLogs {
        resource: Some(resource(function_context, config)),
        scope_logs,
        schema_url: "".to_owned(),
    }
}

pub fn wrap_spans_in_scope(
    spans: Vec<otel_trace::Span>,
    function_context: &FunctionContext,
) -> ScopeSpans {
    ScopeSpans {
        scope: Some(instrumentation_scope(function_context)),
        spans,
        schema_url: "".to_owned(),
    }
}

pub fn wrap_spans_in_resource(
    scope_spans: Vec<ScopeSpans>,
    function_context: &FunctionContext,
    config: &AttributeConfig,
) -> ResourceSpans {
    ResourceSpans {
        resource: Some(resource(function_context, config)),
        scope_spans,
        schema_url: "".to_owned(),
    }
}

pub fn wrap_metrics_in_scope(metrics: Vec<otel_metrics::Metric>) -> ScopeMetrics {
    ScopeMetrics {
        scope: None, // The scope is ignored by metrics-gateway anyway
        metrics,
        schema_url: "".to_owned(),
    }
}

pub fn wrap_metrics_in_resource(
    scope_metrics: Vec<ScopeMetrics>,
    function_context: &FunctionContext,
    config: &AttributeConfig,
) -> ResourceMetrics {
    ResourceMetrics {
        resource: Some(resource(function_context, config)),
        scope_metrics,
        schema_url: "".to_owned(),
    }
}

fn resource(function_context: &FunctionContext, config: &AttributeConfig) -> Resource {
    let mut attributes = vec![
        string_attribute("cx.application.name", &function_context.application_name),
        string_attribute("cx.subsystem.name", &function_context.subsystem_name),
    ];

    if config.built_in.service_name {
        attributes.push(string_attribute(
            "service.name",
            &function_context.service_name,
        ));
    }

    if config.built_in.cloud_provider {
        // https://github.com/open-telemetry/semantic-conventions/blob/v1.32.0/docs/resource/cloud.md?plain=1#L22
        attributes.push(string_attribute("cloud.provider", "aws"));
    }

    if config.built_in.cloud_account_id {
        // https://github.com/open-telemetry/semantic-conventions/blob/v1.32.0/docs/resource/cloud.md?plain=1#L19
        attributes.push(string_attribute(
            "cloud.account.id",
            &function_context.arn.account_id,
        ));
    }

    if config.built_in.cloud_region {
        // https://github.com/open-telemetry/semantic-conventions/blob/v1.32.0/docs/resource/cloud.md?plain=1#L23
        attributes.push(string_attribute(
            "cloud.region",
            &function_context.arn.region,
        ));
    }

    if config.built_in.faas_name {
        // https://github.com/open-telemetry/semantic-conventions/blob/v1.32.0/docs/resource/faas.md?plain=1#L30
        attributes.push(string_attribute(
            "faas.name",
            &function_context.arn.function_name,
        ));
    }

    if config.built_in.faas_id {
        // https://github.com/open-telemetry/opentelemetry-specification/blob/v1.17.0/specification/resource/semantic_conventions/faas.md?plain=1#L20
        // This has changed since and our implementation is out-of-date with the latest conventions https://github.com/open-telemetry/semantic-conventions/blob/v1.32.0/docs/resource/faas.md?plain=1#L31
        attributes.push(string_attribute(
            "faas.id",
            &function_context.version_arn.to_string(),
        ));
    }

    if config.built_in.faas_instance_cx_id {
        attributes.push(string_attribute(
            "faas.instance.cx_id",
            &function_context.lambda_instance_coralogix_id,
        ));
    }

    if let Some(tags) = function_context.tags.as_ref() {
        attributes.extend(
            tags.iter()
                .map(|tag| string_attribute(&format!("tag.{}", tag.0), tag.1)),
        )
    }

    for (key, value) in &config.extra {
        attributes.push(string_attribute(key, value));
    }

    Resource {
        attributes,
        dropped_attributes_count: 0,
    }
}

fn instrumentation_scope(function_context: &FunctionContext) -> InstrumentationScope {
    InstrumentationScope {
        name: function_context.arn.function_name.clone(),
        version: function_context.lambda_function_version.clone(),
        attributes: Vec::new(),
        dropped_attributes_count: 0,
    }
}
