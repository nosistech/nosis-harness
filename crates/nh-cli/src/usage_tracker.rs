use std::sync::{Arc, RwLock};

use nh_core::wire::{ChatClient, ChatRequest, ChatResponse, RetryExhausted, Usage};

/// Captures only the latest provider request's usage block. Session and task
/// receipts are cumulative, so they cannot honestly drive context occupancy.
#[derive(Clone, Default)]
pub(crate) struct LastRequestUsage {
    value: Arc<RwLock<Option<Usage>>>,
}

impl LastRequestUsage {
    pub(crate) fn wrap(&self, client: Box<dyn ChatClient>) -> Box<dyn ChatClient> {
        self.store(None);
        Box::new(UsageTrackingClient {
            client,
            latest: self.clone(),
        })
    }

    pub(crate) fn snapshot(&self) -> Option<Usage> {
        match self.value.read() {
            Ok(value) => value.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn store(&self, usage: Option<Usage>) {
        match self.value.write() {
            Ok(mut value) => *value = usage,
            Err(poisoned) => *poisoned.into_inner() = usage,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_for_test(&self, usage: Option<Usage>) {
        self.store(usage);
    }
}

struct UsageTrackingClient {
    client: Box<dyn ChatClient>,
    latest: LastRequestUsage,
}

impl ChatClient for UsageTrackingClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let result = self.client.complete(request);
        let usage = match &result {
            Ok(response) => response.usage.clone(),
            Err(error) => error
                .downcast_ref::<RetryExhausted>()
                .filter(|failure| failure.attempts == 1)
                .and_then(|failure| failure.usage.clone()),
        };
        self.latest.store(usage);
        result
    }
}
