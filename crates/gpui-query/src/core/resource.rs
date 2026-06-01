use serde::{Deserialize, Serialize};

use super::{
    CachePolicy, QueryError, QueryKey, QuerySignal, QueryStatus, QueryTimestamp, RequestId,
    RequestPolicy, RetryPolicy,
};

mod accessors;
mod cache;
mod completion;
mod lifecycle;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryResource<T, E = QueryError> {
    key: QueryKey,
    status: QueryStatus,
    data: Option<T>,
    error: Option<E>,
    active_request_id: Option<RequestId>,
    cache_policy: CachePolicy,
    request_policy: RequestPolicy,
    started_at: Option<QueryTimestamp>,
    last_updated_at: Option<QueryTimestamp>,
    cache_hits: u64,
    cancelled_count: u64,
    ignored_results: u64,
    retry_count: u32,
    retry_policy: RetryPolicy,
    placeholder_data: Option<T>,
    previous_data: Option<T>,
    #[serde(skip)]
    initial_data: Option<T>,
    #[serde(skip)]
    signal: Option<QuerySignal>,
}

impl<T, E> QueryResource<T, E> {
    pub fn new(
        key: impl Into<QueryKey>,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
    ) -> Self {
        Self {
            key: key.into(),
            status: QueryStatus::Idle,
            data: None,
            error: None,
            active_request_id: None,
            cache_policy,
            request_policy,
            started_at: None,
            last_updated_at: None,
            cache_hits: 0,
            cancelled_count: 0,
            ignored_results: 0,
            retry_count: 0,
            retry_policy: RetryPolicy::no_retries(),
            placeholder_data: None,
            previous_data: None,
            initial_data: None,
            signal: None,
        }
    }
}
