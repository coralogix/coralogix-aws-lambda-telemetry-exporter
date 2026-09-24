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

use std::{env, fmt};
use tracing::{Event, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::{Format, Full, Writer};
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

pub fn configure_tracing() {
    let env_var = env::var("CX_DIAGNOSTIC_LOG")
        .or_else(|_| env::var("CORALOGIX_DIAGNOSTIC_LOG"))
        .unwrap_or_default();

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .parse_lossy(env_var);

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .event_format(CustomFormat::new())
        .init();
}

struct CustomFormat {
    inner: Format<Full, ()>,
}

impl CustomFormat {
    fn new() -> Self {
        Self {
            inner: Format::default()
                .with_ansi(false) // These logs will be ingested by CloudWatch => no ANSI support
                .with_target(false) // we disable the default target which is the full module name and instead write a short "coralogix: "
                .without_time(), // CloudWatch provides it's own timestamp
        }
    }
}

impl<S, N> FormatEvent<S, N> for CustomFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        writer.write_str("coralogix: ")?;
        self.inner.format_event(ctx, writer, event)
    }
}
