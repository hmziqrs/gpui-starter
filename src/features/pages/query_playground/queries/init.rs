use gpui::*;

use gpui_query::core::{CachePolicy, QueryError, RequestPolicy, RetryPolicy, SelectTransform};
use gpui_query::hook::{
    InfiniteQueryOptions, QueryOptions, use_infinite_query, use_mutation, use_query,
    use_query_select,
};

use crate::services::tokio_runtime::TokioRuntimeGlobal;

use super::actions::run_http;
use super::super::{HttpFetchKind, PlaygroundPage, PlaygroundUser, QueryPlaygroundPage};

// ---------------------------------------------------------------------------
// Lazy init helpers — each sets up the entity on first call
// ---------------------------------------------------------------------------

impl QueryPlaygroundPage {
    pub(super) fn ensure_simple_query(&mut self, cx: &mut Context<Self>) {
        if self.simple_query.is_some() {
            return;
        }
        let exec = cx.background_executor().clone();
        let (entity, sub) = use_query(
            QueryOptions::new("playground::simple")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            move |_signal| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_secs(1)).await;
                    Ok(PlaygroundUser {
                        id: 1,
                        name: "Alice".into(),
                        email: "alice@test.com".into(),
                    })
                }
            },
            cx,
        );
        self.simple_query = Some((entity, sub));
    }

    pub(super) fn ensure_nocache_query(&mut self, cx: &mut Context<Self>) {
        if self.nocache_query.is_some() {
            return;
        }
        let exec = cx.background_executor().clone();
        let (entity, sub) = use_query(
            QueryOptions::new("playground::nocache")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            move |_signal| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_millis(500)).await;
                    Ok(PlaygroundUser {
                        id: 2,
                        name: "NoCache Bob".into(),
                        email: "bob@test.com".into(),
                    })
                }
            },
            cx,
        );
        self.nocache_query = Some((entity, sub));
    }

    pub(super) fn ensure_ttl_query(&mut self, cx: &mut Context<Self>) {
        if self.ttl_query.is_some() {
            return;
        }
        let exec = cx.background_executor().clone();
        let (entity, sub) = use_query(
            QueryOptions::new("playground::ttl")
                .cache_policy(CachePolicy::Ttl { ttl_ms: 5_000 })
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            move |_signal| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_millis(500)).await;
                    Ok(PlaygroundUser {
                        id: 3,
                        name: "TTL Carol".into(),
                        email: "carol@test.com".into(),
                    })
                }
            },
            cx,
        );
        self.ttl_query = Some((entity, sub));
    }

    pub(super) fn ensure_swr_query(&mut self, cx: &mut Context<Self>) {
        if self.swr_query.is_some() {
            return;
        }
        let exec = cx.background_executor().clone();
        let (entity, sub) = use_query(
            QueryOptions::new("playground::swr")
                .cache_policy(CachePolicy::StaleWhileRevalidate {
                    ttl_ms: 3_000,
                    stale_ms: 7_000,
                })
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            move |_signal| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_millis(500)).await;
                    Ok(PlaygroundUser {
                        id: 4,
                        name: "SWR Dave".into(),
                        email: "dave@test.com".into(),
                    })
                }
            },
            cx,
        );
        self.swr_query = Some((entity, sub));
    }

    pub(super) fn ensure_latest_wins_query(&mut self, cx: &mut Context<Self>) {
        if self.latest_wins_query.is_some() {
            return;
        }
        let exec = cx.background_executor().clone();
        let (entity, sub) = use_query(
            QueryOptions::new("playground::latest_wins")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            move |_signal| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_secs(2)).await;
                    Ok("latest-wins result".into())
                }
            },
            cx,
        );
        self.latest_wins_query = Some((entity, sub));
    }

    pub(super) fn ensure_ignore_query(&mut self, cx: &mut Context<Self>) {
        if self.ignore_query.is_some() {
            return;
        }
        let exec = cx.background_executor().clone();
        let (entity, sub) = use_query(
            QueryOptions::new("playground::ignore")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::IgnoreWhileLoading)
                .retry_policy(RetryPolicy::no_retries()),
            move |_signal| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_secs(2)).await;
                    Ok("ignore result".into())
                }
            },
            cx,
        );
        self.ignore_query = Some((entity, sub));
    }

    pub(super) fn ensure_retry_query(&mut self, cx: &mut Context<Self>) {
        if self.retry_query.is_some() {
            return;
        }
        let exec = cx.background_executor().clone();
        let (entity, sub) = use_query(
            QueryOptions::new("playground::retry")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::new(3).with_delay(400)),
            move |_signal| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_millis(300)).await;
                    Err(QueryError::response("simulated failure"))
                }
            },
            cx,
        );
        // Retry attempts don't change query status (it stays Loading), so the
        // status-deduped QueryObserver won't re-render on each increment — the
        // climbing count would be invisible. Attach a raw observer so the retry
        // counter is visible during the retry loop.
        self.retry_query = Some((entity, sub));
    }

    pub(super) fn ensure_mutation(&mut self, cx: &mut Context<Self>) {
        if self.mutation_entity.is_some() {
            return;
        }
        let (entity, sub) = use_mutation((), cx);
        self.mutation_entity = Some((entity, sub));
    }

    pub(super) fn ensure_infinite(&mut self, cx: &mut Context<Self>) {
        if self.infinite_entity.is_some() {
            return;
        }
        let exec = cx.background_executor().clone();
        let (entity, sub) = use_infinite_query(
            InfiniteQueryOptions::new("playground::infinite")
                .max_pages(3)
                .cache_policy(CachePolicy::NoCache)
                .retry_policy(RetryPolicy::no_retries()),
            move |last_page: Option<&PlaygroundPage>| {
                let exec = exec.clone();
                let page_num = last_page.map(|p| p.page_number + 1).unwrap_or(5);
                async move {
                    exec.timer(std::time::Duration::from_millis(600)).await;
                    let items: Vec<String> = (0..5)
                        .map(|i| format!("page {} item {}", page_num, i))
                        .collect();
                    Ok((
                        PlaygroundPage {
                            items,
                            page_number: page_num,
                        },
                        page_num < 10,
                    ))
                }
            },
            cx,
        );
        self.infinite_entity = Some((entity, sub));
    }

    pub(super) fn ensure_select(&mut self, cx: &mut Context<Self>) {
        if self.select_source.is_some() {
            return;
        }
        let transform = SelectTransform::new(|users: &Vec<PlaygroundUser>| {
            users.iter().map(|u| u.name.clone()).collect()
        });
        let exec = cx.background_executor().clone();
        let (mapped, source, subs) = use_query_select(
            QueryOptions::new("playground::select")
                .cache_policy(CachePolicy::NoCache)
                .retry_policy(RetryPolicy::no_retries()),
            transform,
            move |_signal| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_secs(1)).await;
                    Ok(vec![
                        PlaygroundUser {
                            id: 10,
                            name: "Eve".into(),
                            email: "eve@test.com".into(),
                        },
                        PlaygroundUser {
                            id: 11,
                            name: "Frank".into(),
                            email: "frank@test.com".into(),
                        },
                        PlaygroundUser {
                            id: 12,
                            name: "Grace".into(),
                            email: "grace@test.com".into(),
                        },
                    ])
                }
            },
            cx,
        );
        self.select_source = Some(source);
        self.select_mapped = Some(mapped);
        self._select_subs = Some(subs);
    }

    pub(super) fn ensure_imperative(&mut self, cx: &mut Context<Self>) {
        if self.imperative_query.is_some() {
            return;
        }
        let exec = cx.background_executor().clone();
        let (entity, sub) = use_query(
            QueryOptions::new("playground::imperative")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            move |_signal| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_secs(2)).await;
                    Ok("imperative result".into())
                }
            },
            cx,
        );
        self.imperative_query = Some((entity, sub));
    }

    /// Real HTTP query (reqwest over the tokio runtime). Created lazily on the
    /// first HTTP button click — never auto-fetched on page load. The initial
    /// fetcher does GET JSON; `fetch_http` re-fetches with the chosen kind.
    pub(super) fn ensure_http_query(&mut self, cx: &mut Context<Self>) {
        if self.http_query.is_some() {
            return;
        }
        // reqwest must run on tokio; clone the shared client + runtime out of the
        // global (releasing the cx borrow) before entering the fetch closure.
        let (client, runtime) = match cx.try_global::<TokioRuntimeGlobal>() {
            Some(g) => (g.0.http_client.clone(), g.0.runtime.clone()),
            None => return, // fetch_http also guards and logs this
        };
        let (entity, sub) = use_query(
            QueryOptions::new("playground::http")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            move |signal| {
                let client = client.clone();
                let runtime = runtime.clone();
                async move {
                    if signal.is_cancelled() {
                        return Err(QueryError::cancelled("cancelled before send"));
                    }
                    let result = run_http(&client, &runtime, HttpFetchKind::GetJson).await?;
                    Ok(result)
                }
            },
            cx,
        );
        self.http_query = Some((entity, sub));
    }
}
