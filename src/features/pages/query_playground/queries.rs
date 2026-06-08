
use gpui::prelude::*;
use gpui::*;


use gpui_query_v2::core::{
    CachePolicy,
    QueryError, RequestPolicy, RetryPolicy, SelectTransform,
};
use gpui_query_v2::hook::{
    fetch_next_page_infinite, fetch_previous_page_infinite, fetch_query_with_signal, mutate,
    mutate_with_callbacks, use_infinite_query, use_mutation, use_query, use_query_select,
    InfiniteQueryOptions, MutationCallbacks, QueryOptions,
};

use super::{PlaygroundPage, PlaygroundUser, QueryPlaygroundPage};

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
                .retry_policy(RetryPolicy::new(3).with_delay(200)),
            move |_signal| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_millis(300)).await;
                    Err(QueryError::response("simulated failure"))
                }
            },
            cx,
        );
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
                let page_num = last_page
                    .map(|p| p.page_number + 1)
                    .unwrap_or(0);
                async move {
                    exec.timer(std::time::Duration::from_millis(600)).await;
                    let items: Vec<String> = (0..5)
                        .map(|i| format!("page {} item {}", page_num, i))
                        .collect();
                    Ok((PlaygroundPage { items, page_number: page_num }, page_num < 10))
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
                        PlaygroundUser { id: 10, name: "Eve".into(), email: "eve@test.com".into() },
                        PlaygroundUser { id: 11, name: "Frank".into(), email: "frank@test.com".into() },
                        PlaygroundUser { id: 12, name: "Grace".into(), email: "grace@test.com".into() },
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
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

impl QueryPlaygroundPage {
    pub(super) fn fetch_simple(&mut self, cx: &mut Context<Self>) {
        self.ensure_simple_query(cx);
        // Finding 5: Use defensive `if let Some` instead of `unwrap()`.
        let Some((entity, _)) = self.simple_query.as_ref() else {
            return;
        };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        self.log("Simple: fetch triggered");
        fetch_query_with_signal(&entity, move |_signal| {
            let exec = exec.clone();
            async move {
                exec.timer(std::time::Duration::from_secs(1)).await;
                Ok(PlaygroundUser { id: 1, name: "Alice".into(), email: "alice@test.com".into() })
            }
        }, cx);
    }

    pub(super) fn cancel_simple(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.simple_query {
            entity.update(cx, |r, _| {
                if let Some(s) = r.signal() {
                    s.cancel();
                }
            });
            self.log("Simple: signal cancelled");
            // Finding 7: Removed redundant cx.notify() — the QueryObserver
            // already triggers re-render on status change.
        }
    }

    pub(super) fn reset_simple(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.simple_query {
            entity.update(cx, |r, _| r.reset());
            self.log("Simple: reset");
            cx.notify();
        }
    }

    pub(super) fn fetch_nocache(&mut self, cx: &mut Context<Self>) {
        self.ensure_nocache_query(cx);
        let Some((entity, _)) = self.nocache_query.as_ref() else { return };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        fetch_query_with_signal(&entity, move |_signal| {
            let exec = exec.clone();
            async move {
                exec.timer(std::time::Duration::from_millis(500)).await;
                Ok(PlaygroundUser { id: 2, name: "NoCache Bob".into(), email: "bob@test.com".into() })
            }
        }, cx);
    }

    pub(super) fn fetch_ttl(&mut self, cx: &mut Context<Self>) {
        self.ensure_ttl_query(cx);
        let Some((entity, _)) = self.ttl_query.as_ref() else { return };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        fetch_query_with_signal(&entity, move |_signal| {
            let exec = exec.clone();
            async move {
                exec.timer(std::time::Duration::from_millis(500)).await;
                Ok(PlaygroundUser { id: 3, name: "TTL Carol".into(), email: "carol@test.com".into() })
            }
        }, cx);
    }

    pub(super) fn fetch_swr(&mut self, cx: &mut Context<Self>) {
        self.ensure_swr_query(cx);
        let Some((entity, _)) = self.swr_query.as_ref() else { return };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        fetch_query_with_signal(&entity, move |_signal| {
            let exec = exec.clone();
            async move {
                exec.timer(std::time::Duration::from_millis(500)).await;
                Ok(PlaygroundUser { id: 4, name: "SWR Dave".into(), email: "dave@test.com".into() })
            }
        }, cx);
    }

    pub(super) fn spam_latest_wins(&mut self, cx: &mut Context<Self>) {
        self.ensure_latest_wins_query(cx);
        // Finding 5: Defensive `if let Some` instead of `unwrap()`.
        let Some((entity, _)) = self.latest_wins_query.as_ref() else {
            return;
        };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        self.log("LatestWins: spamming 5 fetches");
        for i in 0..5 {
            let label = format!("attempt-{}", i);
            let e = entity.clone();
            let exec = exec.clone();
            fetch_query_with_signal(&e, move |_signal| {
                let l = label.clone();
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_secs(2)).await;
                    Ok(l)
                }
            }, cx);
        }
    }

    pub(super) fn spam_ignore(&mut self, cx: &mut Context<Self>) {
        self.ensure_ignore_query(cx);
        // Finding 5: Defensive `if let Some` instead of `unwrap()`.
        let Some((entity, _)) = self.ignore_query.as_ref() else {
            return;
        };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        self.log("IgnoreWhileLoading: spamming 5 fetches");
        for i in 0..5 {
            let label = format!("attempt-{}", i);
            let e = entity.clone();
            let exec = exec.clone();
            fetch_query_with_signal(&e, move |_signal| {
                let l = label.clone();
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_secs(2)).await;
                    Ok(l)
                }
            }, cx);
        }
    }

    pub(super) fn trigger_failing_fetch(&mut self, cx: &mut Context<Self>) {
        self.ensure_retry_query(cx);
        let Some((entity, _)) = self.retry_query.as_ref() else { return };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        self.log("Retry: triggered failing fetch (3 retries, 200ms backoff)");
        fetch_query_with_signal(&entity, move |_signal| {
            let exec = exec.clone();
            async move {
                exec.timer(std::time::Duration::from_millis(300)).await;
                Err(QueryError::response("simulated failure"))
            }
        }, cx);
    }

    pub(super) fn do_mutate(&mut self, cx: &mut Context<Self>) {
        self.ensure_mutation(cx);
        let Some((entity, _)) = self.mutation_entity.as_ref() else { return };
        let entity = entity.clone();
        // Finding 1/8: Read mutation input from the editable InputState.
        let vars = self.mutation_input_value(cx);
        let vars = if vars.is_empty() {
            "default-vars".to_string()
        } else {
            vars
        };
        let exec = cx.background_executor().clone();
        self.log(format!("Mutation: mutate('{}')", vars));
        mutate(&entity, vars, move |v| {
            let exec = exec.clone();
            async move {
                exec.timer(std::time::Duration::from_secs(1)).await;
                Ok(format!("result for: {}", v))
            }
        }, cx);
    }

    pub(super) fn do_mutate_with_callbacks(&mut self, cx: &mut Context<Self>) {
        self.ensure_mutation(cx);
        let Some((entity, _)) = self.mutation_entity.as_ref() else { return };
        let entity = entity.clone();
        // Finding 1/8: Read mutation input from the editable InputState.
        let vars = self.mutation_input_value(cx);
        let vars = if vars.is_empty() {
            "callback-vars".to_string()
        } else {
            vars
        };
        self.log(format!("Mutation: mutate_with_callbacks('{}')", vars));

        // Finding 2: Use the struct's own `_callback_log` Arc so callbacks
        // write to a log that survives past this method's return.
        let exec = cx.background_executor().clone();
        let log_for_success = self._callback_log.clone();
        let log_for_error = self._callback_log.clone();
        let callbacks = MutationCallbacks::new()
            .on_success(move |data: &String| {
                log_for_success.lock().unwrap().push(format!("on_success: {}", data));
            })
            .on_error(move |err: &QueryError| {
                log_for_error.lock().unwrap().push(format!("on_error: {}", err));
            });

        mutate_with_callbacks(&entity, vars, move |v| {
            let exec = exec.clone();
            async move {
                exec.timer(std::time::Duration::from_secs(1)).await;
                Ok(format!("callback result for: {}", v))
            }
        }, callbacks, cx);

        self.log("Mutation: callbacks registered (on_success, on_error)");
    }

    pub(super) fn reset_mutation(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.mutation_entity {
            entity.update(cx, |r, _| r.reset());
            self.log("Mutation: reset");
            cx.notify();
        }
    }

    pub(super) fn load_next_page(&mut self, cx: &mut Context<Self>) {
        self.ensure_infinite(cx);
        let Some((entity, _)) = self.infinite_entity.as_ref() else { return };
        let entity = entity.clone();
        // Finding 4: Derive page count from entity state instead of tracking
        // a separate `next_page` counter that goes out of sync after resets.
        let current_pages = entity.read_with(cx, |r, _| r.page_count());
        let exec = cx.background_executor().clone();
        self.log(format!("Infinite: load next page (current pages: {})", current_pages));
        fetch_next_page_infinite(&entity, move |last_page: Option<&PlaygroundPage>| {
            let exec = exec.clone();
            let page_num = last_page.map(|p| p.page_number + 1).unwrap_or(0);
            async move {
                exec.timer(std::time::Duration::from_millis(600)).await;
                let items: Vec<String> = (0..5)
                    .map(|i| format!("page {} item {}", page_num, i))
                    .collect();
                Ok((PlaygroundPage { items, page_number: page_num }, page_num < 10))
            }
        }, cx);
    }

    pub(super) fn load_prev_page(&mut self, cx: &mut Context<Self>) {
        self.ensure_infinite(cx);
        let Some((entity, _)) = self.infinite_entity.as_ref() else { return };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        self.log("Infinite: load previous page");
        fetch_previous_page_infinite(&entity, move |first_page: Option<&PlaygroundPage>| {
            let exec = exec.clone();
            let page_num = first_page.map(|p| p.page_number.saturating_sub(1)).unwrap_or(0);
            async move {
                exec.timer(std::time::Duration::from_millis(600)).await;
                let items: Vec<String> = (0..5)
                    .map(|i| format!("page {} item {}", page_num, i))
                    .collect();
                Ok((PlaygroundPage { items, page_number: page_num }, true))
            }
        }, cx);
    }

    pub(super) fn reset_infinite(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.infinite_entity {
            entity.update(cx, |r, _| r.reset());
            // Finding 4: No separate `next_page` counter to reset.
            self.log("Infinite: reset");
            cx.notify();
        }
    }

    pub(super) fn fetch_select(&mut self, cx: &mut Context<Self>) {
        self.ensure_select(cx);
        let source = self.select_source.clone();
        if let Some(source) = source {
            let exec = cx.background_executor().clone();
            fetch_query_with_signal(&source, move |_signal| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_secs(1)).await;
                    Ok(vec![
                        PlaygroundUser { id: 10, name: "Eve".into(), email: "eve@test.com".into() },
                        PlaygroundUser { id: 11, name: "Frank".into(), email: "frank@test.com".into() },
                        PlaygroundUser { id: 12, name: "Grace".into(), email: "grace@test.com".into() },
                    ])
                }
            }, cx);
            self.log("Select: source fetch triggered");
        }
    }

    pub(super) fn fetch_imperative(&mut self, cx: &mut Context<Self>) {
        self.ensure_imperative(cx);
        let Some((entity, _)) = self.imperative_query.as_ref() else { return };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        self.log("Imperative: fetch triggered");
        fetch_query_with_signal(&entity, move |_signal| {
            let exec = exec.clone();
            async move {
                exec.timer(std::time::Duration::from_secs(2)).await;
                Ok("imperative result".into())
            }
        }, cx);
    }

    pub(super) fn cancel_imperative(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.imperative_query {
            entity.update(cx, |r, _| {
                if let Some(s) = r.signal() {
                    s.cancel();
                }
            });
            self.log("Imperative: signal cancelled mid-flight");
            // Finding 7: Removed redundant cx.notify() — the QueryObserver
            // already triggers re-render on status change.
        }
    }

    pub(super) fn reset_imperative(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.imperative_query {
            entity.update(cx, |r, _| r.reset());
            self.log("Imperative: reset");
            cx.notify();
        }
    }
}
