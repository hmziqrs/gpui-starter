use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

use tokio_util::sync::CancellationToken;

use gpui_query_legacy::RequestId;

const LOG: &str = "gpui_starter::http_lab::tasks";

pub(super) fn cancellation_flags() -> &'static Mutex<BTreeMap<RequestId, CancellationToken>> {
    static FLAGS: OnceLock<Mutex<BTreeMap<RequestId, CancellationToken>>> = OnceLock::new();
    FLAGS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub(super) fn register_request_flag(request_id: RequestId) -> CancellationToken {
    let flag = CancellationToken::new();
    if let Ok(mut flags) = cancellation_flags().lock() {
        flags.insert(request_id, flag.clone());
        tracing::debug!(
            target: LOG,
            request_id = %request_id.label(),
            active_tokens = flags.len(),
            "HTTP Lab cancellation token registered"
        );
    }
    flag
}

pub(super) fn cancel_request_flag(request_id: RequestId) {
    if let Ok(flags) = cancellation_flags().lock()
        && let Some(flag) = flags.get(&request_id)
    {
        flag.cancel();
        tracing::info!(
            target: LOG,
            request_id = %request_id.label(),
            "HTTP Lab cancellation token cancelled"
        );
    }
}

pub(super) fn remove_request_flag(request_id: RequestId) {
    if let Ok(mut flags) = cancellation_flags().lock() {
        flags.remove(&request_id);
        tracing::debug!(
            target: LOG,
            request_id = %request_id.label(),
            active_tokens = flags.len(),
            "HTTP Lab cancellation token removed"
        );
    }
}
