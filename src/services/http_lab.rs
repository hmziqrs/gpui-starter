mod client;
mod operations;
mod response;
mod state;
mod task_tracking;
mod transitions;
mod types;

pub use operations::{
    ActionHandle, cancel_action, cancel_all, execute_action, initialize, prefetch_action,
    prepare_action, read_state, reset, select_action, snapshot,
};
pub use response::response_fields;
pub use state::{HttpLabDiagnostic, HttpLabState};
pub use types::{
    HttpBodyKind, HttpCookieSnapshot, HttpExchange, HttpLabAction, HttpRequestBodyKind,
    HttpRequestSnapshot, HttpResponseSnapshot,
};

#[cfg(test)]
mod test_support;

#[cfg(test)]
#[path = "http_lab/cache.test.rs"]
mod http_lab_cache_test;

#[cfg(test)]
#[path = "http_lab/flow.test.rs"]
mod http_lab_flow_test;

#[cfg(test)]
#[path = "http_lab/response.test.rs"]
mod http_lab_response_test;

#[cfg(test)]
#[path = "http_lab/tasks.test.rs"]
mod http_lab_tasks_test;

#[cfg(test)]
#[path = "http_lab/data_retention.test.rs"]
mod http_lab_data_retention_test;

#[cfg(test)]
#[path = "http_lab/optimistic.test.rs"]
mod http_lab_optimistic_test;

#[cfg(test)]
#[path = "http_lab/signal.test.rs"]
mod http_lab_signal_test;

#[cfg(test)]
#[path = "http_lab/retry.test.rs"]
mod http_lab_retry_test;

#[cfg(test)]
#[path = "http_lab/client_features.test.rs"]
mod http_lab_client_features_test;
