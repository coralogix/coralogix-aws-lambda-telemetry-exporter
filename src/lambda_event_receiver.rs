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

use lambda_extension::{Error, LambdaEvent, NextEvent};

use futures::Future;
use lambda_extension;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::Service;

use crate::error_handling::ErrorHandlingMode;
use crate::telemetry::telemetry_service::TelemetryService;

#[derive(Clone)]
pub struct LambdaEventReceiver {
    telemetry_service: Arc<TelemetryService>,
    error_handling: ErrorHandlingMode,
}

impl LambdaEventReceiver {
    pub fn new(
        telemetry_service: Arc<TelemetryService>,
        error_handling: ErrorHandlingMode,
    ) -> LambdaEventReceiver {
        LambdaEventReceiver {
            telemetry_service,
            error_handling,
        }
    }

    async fn handle_lambda_event(&self, event: LambdaEvent) -> Result<(), Error> {
        let result = match event.next {
            NextEvent::Shutdown(e) => self.telemetry_service.handle_shutdown_event(e).await,
            NextEvent::Invoke(e) => self.telemetry_service.handle_invoke_event(e).await,
        };
        if let Err(e) = result {
            let msg = format!("Failed to handle a lambda event: {e}");
            self.error_handling.handle_error(&msg);
        }
        Ok(())
    }
}

impl Service<LambdaEvent> for LambdaEventReceiver {
    type Response = ();
    type Error = lambda_extension::Error;
    type Future = Pin<Box<dyn Future<Output = Result<(), Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: LambdaEvent) -> Self::Future {
        let clone = self.clone();
        Box::pin(async move { clone.handle_lambda_event(req).await })
    }
}
