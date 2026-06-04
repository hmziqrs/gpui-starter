//! Query V2 Playground — interactive demo of every gpui-query-v2 feature.
//!
//! Each section exercises a distinct capability: simple queries, cache policies,
//! request policies, retry, mutations (with callbacks), infinite queries,
//! select transforms, and imperative fetch with signal cancellation.

use std::sync::Arc;

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
    input::{Input, InputEvent, InputState},
};
use serde::{Deserialize, Serialize};

use gpui_query_v2::client::QueryClient;
use gpui_query_v2::core::{
    CachePolicy, InfiniteQueryResource, MappedQueryResource, MutationResource, MutationStatus,
    QueryError, QueryResource, QueryStatus, RequestPolicy, RetryPolicy, SelectTransform,
};
use gpui_query_v2::hook::{
    fetch_next_page_infinite, fetch_previous_page_infinite, fetch_query_with_signal, mutate,
    mutate_with_callbacks, use_infinite_query, use_mutation, use_query, use_query_select,
    InfiniteQueryOptions, MutationCallbacks, QueryOptions,
};

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaygroundUser {
    pub id: u32,
    pub name: String,
    pub email: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaygroundPage {
    pub items: Vec<String>,
    pub page_number: u32,
}

// ---------------------------------------------------------------------------
// Page struct
// ---------------------------------------------------------------------------

pub struct QueryPlaygroundPage {
    _subscriptions: Vec<Subscription>,
    // Simple query
    simple_query: Option<(Entity<QueryResource<PlaygroundUser, QueryError>>, Subscription)>,
    // Cache policy demos
    nocache_query: Option<(Entity<QueryResource<PlaygroundUser, QueryError>>, Subscription)>,
    ttl_query: Option<(Entity<QueryResource<PlaygroundUser, QueryError>>, Subscription)>,
    swr_query: Option<(Entity<QueryResource<PlaygroundUser, QueryError>>, Subscription)>,
    // Request policy demos
    latest_wins_query: Option<(Entity<QueryResource<String, QueryError>>, Subscription)>,
    ignore_query: Option<(Entity<QueryResource<String, QueryError>>, Subscription)>,
    // Retry demo
    retry_query: Option<(Entity<QueryResource<String, QueryError>>, Subscription)>,
    // Mutation demo
    mutation_entity: Option<(Entity<MutationResource<String, String, QueryError>>, Subscription)>,
    // Mutation input state
    mutation_input_state: Entity<InputState>,
    // Infinite query
    infinite_entity: Option<(Entity<InfiniteQueryResource<PlaygroundPage, QueryError>>, Subscription)>,
    // Select transform
    select_source: Option<Entity<QueryResource<Vec<PlaygroundUser>, QueryError>>>,
    select_mapped: Option<Entity<MappedQueryResource<Vec<PlaygroundUser>, Vec<String>, QueryError>>>,
    _select_subs: Option<(Subscription, Subscription)>,
    // Imperative fetch
    imperative_query: Option<(Entity<QueryResource<String, QueryError>>, Subscription)>,
    // UI state
    activity_log: Vec<String>,
    // Shared callback log that survives past method returns (Finding 2)
    _callback_log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl QueryPlaygroundPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subs = Vec::new();
        subs.push(
            cx.observe_global_in::<QueryClient>(window, |_, _, cx| {
                cx.notify();
            }),
        );

        // Finding 1/8: Create a proper editable InputState for mutation input.
        let mutation_input_state = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Enter mutation variables...")
        });

        // Subscribe to input changes so the UI re-reads from mutation_input_state.
        subs.push(
            cx.subscribe(
                &mutation_input_state,
                |_this: &mut Self, _state: Entity<InputState>, ev: &InputEvent, cx| {
                    if let InputEvent::Change = ev {
                        cx.notify();
                    }
                },
            ),
        );

        let callback_log = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

        Self {
            _subscriptions: subs,
            simple_query: None,
            nocache_query: None,
            ttl_query: None,
            swr_query: None,
            latest_wins_query: None,
            ignore_query: None,
            retry_query: None,
            mutation_entity: None,
            mutation_input_state,
            infinite_entity: None,
            select_source: None,
            select_mapped: None,
            _select_subs: None,
            imperative_query: None,
            activity_log: Vec::new(),
            _callback_log: callback_log,
        }
    }

    /// Read the current mutation input text from the InputState entity.
    fn mutation_input_value(&self, cx: &App) -> String {
        self.mutation_input_state.read(cx).value().to_string()
    }

    fn log(&mut self, msg: impl Into<String>) {
        self.activity_log.push(msg.into());
        if self.activity_log.len() > 50 {
            self.activity_log.remove(0);
        }
    }

    // -----------------------------------------------------------------------
    // Lazy init helpers — each sets up the entity on first call
    // -----------------------------------------------------------------------

    fn ensure_simple_query(&mut self, cx: &mut Context<Self>) {
        if self.simple_query.is_some() {
            return;
        }
        let (entity, sub) = use_query(
            QueryOptions::new("playground::simple")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            |_signal| async move {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                Ok(PlaygroundUser {
                    id: 1,
                    name: "Alice".into(),
                    email: "alice@test.com".into(),
                })
            },
            cx,
        );
        self.simple_query = Some((entity, sub));
    }

    fn ensure_nocache_query(&mut self, cx: &mut Context<Self>) {
        if self.nocache_query.is_some() {
            return;
        }
        let (entity, sub) = use_query(
            QueryOptions::new("playground::nocache")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            |_signal| async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                Ok(PlaygroundUser {
                    id: 2,
                    name: "NoCache Bob".into(),
                    email: "bob@test.com".into(),
                })
            },
            cx,
        );
        self.nocache_query = Some((entity, sub));
    }

    fn ensure_ttl_query(&mut self, cx: &mut Context<Self>) {
        if self.ttl_query.is_some() {
            return;
        }
        let (entity, sub) = use_query(
            QueryOptions::new("playground::ttl")
                .cache_policy(CachePolicy::Ttl { ttl_ms: 5_000 })
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            |_signal| async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                Ok(PlaygroundUser {
                    id: 3,
                    name: "TTL Carol".into(),
                    email: "carol@test.com".into(),
                })
            },
            cx,
        );
        self.ttl_query = Some((entity, sub));
    }

    fn ensure_swr_query(&mut self, cx: &mut Context<Self>) {
        if self.swr_query.is_some() {
            return;
        }
        let (entity, sub) = use_query(
            QueryOptions::new("playground::swr")
                .cache_policy(CachePolicy::StaleWhileRevalidate {
                    ttl_ms: 3_000,
                    stale_ms: 7_000,
                })
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            |_signal| async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                Ok(PlaygroundUser {
                    id: 4,
                    name: "SWR Dave".into(),
                    email: "dave@test.com".into(),
                })
            },
            cx,
        );
        self.swr_query = Some((entity, sub));
    }

    fn ensure_latest_wins_query(&mut self, cx: &mut Context<Self>) {
        if self.latest_wins_query.is_some() {
            return;
        }
        let (entity, sub) = use_query(
            QueryOptions::new("playground::latest_wins")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            |_signal| async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                Ok("latest-wins result".into())
            },
            cx,
        );
        self.latest_wins_query = Some((entity, sub));
    }

    fn ensure_ignore_query(&mut self, cx: &mut Context<Self>) {
        if self.ignore_query.is_some() {
            return;
        }
        let (entity, sub) = use_query(
            QueryOptions::new("playground::ignore")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::IgnoreWhileLoading)
                .retry_policy(RetryPolicy::no_retries()),
            |_signal| async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                Ok("ignore result".into())
            },
            cx,
        );
        self.ignore_query = Some((entity, sub));
    }

    fn ensure_retry_query(&mut self, cx: &mut Context<Self>) {
        if self.retry_query.is_some() {
            return;
        }
        let (entity, sub) = use_query(
            QueryOptions::new("playground::retry")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::new(3).with_delay(200)),
            |_signal| async move {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                Err(QueryError::response("simulated failure"))
            },
            cx,
        );
        self.retry_query = Some((entity, sub));
    }

    fn ensure_mutation(&mut self, cx: &mut Context<Self>) {
        if self.mutation_entity.is_some() {
            return;
        }
        let (entity, sub) = use_mutation((), cx);
        self.mutation_entity = Some((entity, sub));
    }

    fn ensure_infinite(&mut self, cx: &mut Context<Self>) {
        if self.infinite_entity.is_some() {
            return;
        }
        let (entity, sub) = use_infinite_query(
            InfiniteQueryOptions::new("playground::infinite")
                .max_pages(3)
                .cache_policy(CachePolicy::NoCache)
                .retry_policy(RetryPolicy::no_retries()),
            |last_page: Option<&PlaygroundPage>| {
                let page_num = last_page
                    .map(|p| p.page_number + 1)
                    .unwrap_or(0);
                async move {
                    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
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

    fn ensure_select(&mut self, cx: &mut Context<Self>) {
        if self.select_source.is_some() {
            return;
        }
        let transform = SelectTransform::new(|users: &Vec<PlaygroundUser>| {
            users.iter().map(|u| u.name.clone()).collect()
        });
        let (mapped, source, subs) = use_query_select(
            QueryOptions::new("playground::select")
                .cache_policy(CachePolicy::NoCache)
                .retry_policy(RetryPolicy::no_retries()),
            transform,
            |_signal| async move {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                Ok(vec![
                    PlaygroundUser { id: 10, name: "Eve".into(), email: "eve@test.com".into() },
                    PlaygroundUser { id: 11, name: "Frank".into(), email: "frank@test.com".into() },
                    PlaygroundUser { id: 12, name: "Grace".into(), email: "grace@test.com".into() },
                ])
            },
            cx,
        );
        self.select_source = Some(source);
        self.select_mapped = Some(mapped);
        self._select_subs = Some(subs);
    }

    fn ensure_imperative(&mut self, cx: &mut Context<Self>) {
        if self.imperative_query.is_some() {
            return;
        }
        let (entity, sub) = use_query(
            QueryOptions::new("playground::imperative")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::LatestWins)
                .retry_policy(RetryPolicy::no_retries()),
            |_signal| async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                Ok("imperative result".into())
            },
            cx,
        );
        self.imperative_query = Some((entity, sub));
    }

    // -----------------------------------------------------------------------
    // Actions
    // -----------------------------------------------------------------------

    fn fetch_simple(&mut self, cx: &mut Context<Self>) {
        self.ensure_simple_query(cx);
        // Finding 5: Use defensive `if let Some` instead of `unwrap()`.
        let Some((entity, _)) = self.simple_query.as_ref() else {
            return;
        };
        let entity = entity.clone();
        self.log("Simple: fetch triggered");
        fetch_query_with_signal(&entity, |_signal| async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            Ok(PlaygroundUser { id: 1, name: "Alice".into(), email: "alice@test.com".into() })
        }, cx);
    }

    fn cancel_simple(&mut self, cx: &mut Context<Self>) {
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

    fn reset_simple(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.simple_query {
            entity.update(cx, |r, _| r.reset());
            self.log("Simple: reset");
            cx.notify();
        }
    }

    fn fetch_nocache(&mut self, cx: &mut Context<Self>) {
        self.ensure_nocache_query(cx);
        let Some((entity, _)) = self.nocache_query.as_ref() else { return };
        let entity = entity.clone();
        fetch_query_with_signal(&entity, |_signal| async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            Ok(PlaygroundUser { id: 2, name: "NoCache Bob".into(), email: "bob@test.com".into() })
        }, cx);
    }

    fn fetch_ttl(&mut self, cx: &mut Context<Self>) {
        self.ensure_ttl_query(cx);
        let Some((entity, _)) = self.ttl_query.as_ref() else { return };
        let entity = entity.clone();
        fetch_query_with_signal(&entity, |_signal| async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            Ok(PlaygroundUser { id: 3, name: "TTL Carol".into(), email: "carol@test.com".into() })
        }, cx);
    }

    fn fetch_swr(&mut self, cx: &mut Context<Self>) {
        self.ensure_swr_query(cx);
        let Some((entity, _)) = self.swr_query.as_ref() else { return };
        let entity = entity.clone();
        fetch_query_with_signal(&entity, |_signal| async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            Ok(PlaygroundUser { id: 4, name: "SWR Dave".into(), email: "dave@test.com".into() })
        }, cx);
    }

    fn spam_latest_wins(&mut self, cx: &mut Context<Self>) {
        self.ensure_latest_wins_query(cx);
        // Finding 5: Defensive `if let Some` instead of `unwrap()`.
        let Some((entity, _)) = self.latest_wins_query.as_ref() else {
            return;
        };
        let entity = entity.clone();
        self.log("LatestWins: spamming 5 fetches");
        for i in 0..5 {
            let label = format!("attempt-{}", i);
            let e = entity.clone();
            fetch_query_with_signal(&e, move |_signal| {
                let l = label.clone();
                async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    Ok(l)
                }
            }, cx);
        }
    }

    fn spam_ignore(&mut self, cx: &mut Context<Self>) {
        self.ensure_ignore_query(cx);
        // Finding 5: Defensive `if let Some` instead of `unwrap()`.
        let Some((entity, _)) = self.ignore_query.as_ref() else {
            return;
        };
        let entity = entity.clone();
        self.log("IgnoreWhileLoading: spamming 5 fetches");
        for i in 0..5 {
            let label = format!("attempt-{}", i);
            let e = entity.clone();
            fetch_query_with_signal(&e, move |_signal| {
                let l = label.clone();
                async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    Ok(l)
                }
            }, cx);
        }
    }

    fn trigger_failing_fetch(&mut self, cx: &mut Context<Self>) {
        self.ensure_retry_query(cx);
        let Some((entity, _)) = self.retry_query.as_ref() else { return };
        let entity = entity.clone();
        self.log("Retry: triggered failing fetch (3 retries, 200ms backoff)");
        fetch_query_with_signal(&entity, |_signal| async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            Err(QueryError::response("simulated failure"))
        }, cx);
    }

    fn do_mutate(&mut self, cx: &mut Context<Self>) {
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
        self.log(format!("Mutation: mutate('{}')", vars));
        mutate(&entity, vars, |v| async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            Ok(format!("result for: {}", v))
        }, cx);
    }

    fn do_mutate_with_callbacks(&mut self, cx: &mut Context<Self>) {
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
        let log_for_success = self._callback_log.clone();
        let log_for_error = self._callback_log.clone();
        let callbacks = MutationCallbacks::new()
            .on_success(move |data: &String| {
                log_for_success.lock().unwrap().push(format!("on_success: {}", data));
            })
            .on_error(move |err: &QueryError| {
                log_for_error.lock().unwrap().push(format!("on_error: {}", err));
            });

        mutate_with_callbacks(&entity, vars, |v| async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            Ok(format!("callback result for: {}", v))
        }, callbacks, cx);

        self.log("Mutation: callbacks registered (on_success, on_error)");
    }

    fn reset_mutation(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.mutation_entity {
            entity.update(cx, |r, _| r.reset());
            self.log("Mutation: reset");
            cx.notify();
        }
    }

    fn load_next_page(&mut self, cx: &mut Context<Self>) {
        self.ensure_infinite(cx);
        let Some((entity, _)) = self.infinite_entity.as_ref() else { return };
        let entity = entity.clone();
        // Finding 4: Derive page count from entity state instead of tracking
        // a separate `next_page` counter that goes out of sync after resets.
        let current_pages = entity.read_with(cx, |r, _| r.page_count());
        self.log(format!("Infinite: load next page (current pages: {})", current_pages));
        fetch_next_page_infinite(&entity, |last_page: Option<&PlaygroundPage>| {
            let page_num = last_page.map(|p| p.page_number + 1).unwrap_or(0);
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                let items: Vec<String> = (0..5)
                    .map(|i| format!("page {} item {}", page_num, i))
                    .collect();
                Ok((PlaygroundPage { items, page_number: page_num }, page_num < 10))
            }
        }, cx);
    }

    fn load_prev_page(&mut self, cx: &mut Context<Self>) {
        self.ensure_infinite(cx);
        let Some((entity, _)) = self.infinite_entity.as_ref() else { return };
        let entity = entity.clone();
        self.log("Infinite: load previous page");
        fetch_previous_page_infinite(&entity, |first_page: Option<&PlaygroundPage>| {
            let page_num = first_page.map(|p| p.page_number.saturating_sub(1)).unwrap_or(0);
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                let items: Vec<String> = (0..5)
                    .map(|i| format!("page {} item {}", page_num, i))
                    .collect();
                Ok((PlaygroundPage { items, page_number: page_num }, true))
            }
        }, cx);
    }

    fn reset_infinite(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.infinite_entity {
            entity.update(cx, |r, _| r.reset());
            // Finding 4: No separate `next_page` counter to reset.
            self.log("Infinite: reset");
            cx.notify();
        }
    }

    fn fetch_select(&mut self, cx: &mut Context<Self>) {
        self.ensure_select(cx);
        let source = self.select_source.clone();
        if let Some(source) = source {
            fetch_query_with_signal(&source, |_signal| async move {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                Ok(vec![
                    PlaygroundUser { id: 10, name: "Eve".into(), email: "eve@test.com".into() },
                    PlaygroundUser { id: 11, name: "Frank".into(), email: "frank@test.com".into() },
                    PlaygroundUser { id: 12, name: "Grace".into(), email: "grace@test.com".into() },
                ])
            }, cx);
            self.log("Select: source fetch triggered");
        }
    }

    fn fetch_imperative(&mut self, cx: &mut Context<Self>) {
        self.ensure_imperative(cx);
        let Some((entity, _)) = self.imperative_query.as_ref() else { return };
        let entity = entity.clone();
        self.log("Imperative: fetch triggered");
        fetch_query_with_signal(&entity, |_signal| async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            Ok("imperative result".into())
        }, cx);
    }

    fn cancel_imperative(&mut self, cx: &mut Context<Self>) {
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

    fn reset_imperative(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.imperative_query {
            entity.update(cx, |r, _| r.reset());
            self.log("Imperative: reset");
            cx.notify();
        }
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

impl Render for QueryPlaygroundPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Extract theme colors upfront to release borrows before calling methods.
        let theme = cx.theme();
        let radius_lg = theme.radius_lg;
        let border = theme.border;
        let muted = theme.muted;
        let muted_foreground = theme.muted_foreground;
        let _ = theme;

        let page = v_flex()
            .min_h_full()
            .p_6()
            .gap_5()
            // -- Header --
            .child(
                div()
                    .p_5()
                    .rounded(radius_lg)
                    .border_1()
                    .border_color(border)
                    .bg(muted)
                    .child(
                        v_flex().gap_3()
                            .child(
                                div().text_2xl().font_weight(FontWeight::BOLD)
                                    .child("Query V2 Playground"),
                            )
                            .child(
                                div()
                                    .max_w(px(800.))
                                    .text_sm()
                                    .text_color(muted_foreground)
                                    .child(
                                        "Interactive demo of every gpui-query-v2 feature: queries, \
                                         cache policies, request policies, retry, mutations, \
                                         infinite queries, select transforms, and imperative fetch \
                                         with signal cancellation.",
                                    ),
                            ),
                    ),
            )
            // -- 1. Simple Query --
            .child(self.render_simple_query(cx))
            // -- 2. Cache Policies --
            .child(self.render_cache_policies(cx))
            // -- 3. Request Policies --
            .child(self.render_request_policies(cx))
            // -- 4. Retry Policy --
            .child(self.render_retry_policy(cx))
            // -- 5. Mutation --
            .child(self.render_mutation(cx))
            // -- 6. Infinite Query --
            .child(self.render_infinite_query(cx))
            // -- 7. Select Transform --
            .child(self.render_select_transform(cx))
            // -- 8. Imperative Fetch --
            .child(self.render_imperative_fetch(cx))
            // -- 9. Activity Log --
            .child(self.render_activity_log(cx));

        page
    }
}

// ---------------------------------------------------------------------------
// Section renderers
// ---------------------------------------------------------------------------

impl QueryPlaygroundPage {
    fn render_simple_query(&mut self, cx: &mut Context<Self>) -> Div {
        let loading = self.simple_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });
        let status = self.simple_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let data_preview = self.simple_query.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let cache_age = self.simple_query.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                r.last_updated_at_ms().map(|t| now.saturating_sub(t))
            })
        });
        let bg = cx.theme().background;

        section_card("Simple Query", "Basic fetch with NoCache + LatestWins. Simulates 1s async work.", cx)
            .child(
                h_flex().gap_2().flex_wrap().px_4().py_3()
                    .child(
                        Button::new("pg-simple-fetch")
                            // Finding 11: Use .primary() for the main action button.
                            .primary()
                            .label(if loading { "Fetching..." } else { "Fetch" })
                            .disabled(loading)
                            .on_click(cx.listener(|this, _, _, cx| this.fetch_simple(cx))),
                    )
                    .child(
                        Button::new("pg-simple-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(!loading)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_simple(cx))),
                    )
                    .child(
                        Button::new("pg-simple-reset")
                            .outline()
                            .label("Reset")
                            .on_click(cx.listener(|this, _, _, cx| this.reset_simple(cx))),
                    ),
            )
            .child(
                h_flex().gap_3().items_center().px_4().pb_3()
                    .child(status_badge(status, cx))
                    .when_some(data_preview, |el, user| {
                        el.child(chip(&format!("{} <{}>", user.name, user.email), bg, cx))
                    })
                    .when_some(cache_age, |el, age| {
                        el.child(chip(&format!("age: {}ms", age), bg, cx))
                    }),
            )
    }

    fn render_cache_policies(&mut self, cx: &mut Context<Self>) -> Div {

        // NoCache
        let nocache_status = self.nocache_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let nocache_loading = self.nocache_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });

        // TTL
        let ttl_status = self.ttl_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let ttl_loading = self.ttl_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });

        // SWR
        let swr_status = self.swr_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let swr_loading = self.swr_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });

        section_card("Cache Policies", "Compare NoCache, TTL 5s, and StaleWhileRevalidate 3s/7s.", cx)
            .child(
                h_flex().gap_4().px_4().py_3()
                    // NoCache card
                    .child(
                        mini_card("NoCache", cx)
                            .child(status_badge(nocache_status, cx))
                            .child(
                                Button::new("pg-nocache-fetch")
                                    .primary()
                                    .label(if nocache_loading { "Fetching" } else { "Fetch" })
                                    .disabled(nocache_loading)
                                    .on_click(cx.listener(|this, _, _, cx| this.fetch_nocache(cx))),
                            ),
                    )
                    // TTL card
                    .child(
                        mini_card("TTL 5s", cx)
                            .child(status_badge(ttl_status, cx))
                            .child(
                                Button::new("pg-ttl-fetch")
                                    .primary()
                                    .label(if ttl_loading { "Fetching" } else { "Fetch" })
                                    .disabled(ttl_loading)
                                    .on_click(cx.listener(|this, _, _, cx| this.fetch_ttl(cx))),
                            ),
                    )
                    // SWR card
                    .child(
                        mini_card("SWR 3s/7s", cx)
                            .child(status_badge(swr_status, cx))
                            .child(
                                Button::new("pg-swr-fetch")
                                    .primary()
                                    .label(if swr_loading { "Fetching" } else { "Fetch" })
                                    .disabled(swr_loading)
                                    .on_click(cx.listener(|this, _, _, cx| this.fetch_swr(cx))),
                            ),
                    ),
            )
    }

    fn render_request_policies(&mut self, cx: &mut Context<Self>) -> Div {

        let latest_status = self.latest_wins_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let latest_data = self.latest_wins_query.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let latest_loading = self.latest_wins_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });

        let ignore_status = self.ignore_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let ignore_data = self.ignore_query.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let ignore_loading = self.ignore_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });

        section_card(
            "Request Policies",
            "LatestWins: last fetch wins, older results discarded. IgnoreWhileLoading: first fetch completes, rest ignored.",
            cx,
        )
        .child(
            h_flex().gap_4().px_4().py_3()
                // LatestWins card
                .child(
                    mini_card("LatestWins", cx)
                        .child(status_badge(latest_status, cx))
                        .when_some(latest_data, |el, d| {
                            el.child(chip(&d, cx.theme().background, cx))
                        })
                        .child(
                            Button::new("pg-latest-spam")
                                .primary()
                                .label("Spam Fetch (5x)")
                                .disabled(latest_loading)
                                .on_click(cx.listener(|this, _, _, cx| this.spam_latest_wins(cx))),
                        ),
                )
                // IgnoreWhileLoading card
                .child(
                    mini_card("IgnoreWhileLoading", cx)
                        .child(status_badge(ignore_status, cx))
                        .when_some(ignore_data, |el, d| {
                            el.child(chip(&d, cx.theme().background, cx))
                        })
                        .child(
                            Button::new("pg-ignore-spam")
                                .primary()
                                .label("Spam Fetch (5x)")
                                .disabled(ignore_loading)
                                .on_click(cx.listener(|this, _, _, cx| this.spam_ignore(cx))),
                        ),
                ),
        )
    }

    fn render_retry_policy(&mut self, cx: &mut Context<Self>) -> Div {

        let status = self.retry_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let retry_count = self.retry_query.as_ref().map_or(0, |(e, _)| {
            e.read_with(cx, |r, _| r.retry_count())
        });
        let loading = status.is_loading();
        let policy = RetryPolicy::new(3).with_delay(200);

        section_card(
            "Retry Policy",
            "Fetcher always returns Err. Shows retry count incrementing with exponential backoff (3 retries, 200ms base).",
            cx,
        )
        .child(
            h_flex().gap_2().flex_wrap().px_4().py_3()
                .child(
                    Button::new("pg-retry-trigger")
                        .primary()
                        .label(if loading { "Retrying..." } else { "Trigger Failing Fetch" })
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| this.trigger_failing_fetch(cx))),
                ),
        )
        .child(
            h_flex().gap_3().items_center().px_4().pb_3()
                .child(status_badge(status, cx))
                .child(chip(&format!("retries: {}/{}", retry_count, policy.max_retries), cx.theme().background, cx))
                .child(chip(&format!("backoff: {}ms base", policy.retry_delay_ms), cx.theme().background, cx)),
        )
    }

    fn render_mutation(&mut self, cx: &mut Context<Self>) -> Div {

        let m_status = self.mutation_entity.as_ref().map_or(
            MutationStatus::Idle,
            |(e, _)| e.read_with(cx, |r, _| r.status()),
        );
        let m_data = self.mutation_entity.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let m_error = self.mutation_entity.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.error().cloned())
        });
        let m_vars = self.mutation_entity.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.variables().cloned())
        });
        let m_loading = m_status == MutationStatus::Loading;

        let status_color = match m_status {
            MutationStatus::Idle => cx.theme().muted_foreground,
            MutationStatus::Loading => cx.theme().info,
            MutationStatus::Success => cx.theme().success,
            MutationStatus::Failure => cx.theme().danger,
        };

        section_card(
            "Mutation",
            "Text input for mutation variables. Mutate() fires async; mutate_with_callbacks() also logs on_success/on_error.",
            cx,
        )
        .child(
            h_flex().gap_2().items_center().px_4().py_2()
                // Finding 1/8: Replace static div with a proper editable Input
                // component that binds to mutation_input_state.
                .child(
                    div().min_w(px(200.))
                        .child(Input::new(&self.mutation_input_state))
                )
                .child(
                    Button::new("pg-mutate")
                        .primary()
                        .label(if m_loading { "Mutating..." } else { "Mutate" })
                        .disabled(m_loading)
                        .on_click(cx.listener(|this, _, _, cx| this.do_mutate(cx))),
                )
                .child(
                    Button::new("pg-mutate-cb")
                        .outline()
                        .label("Mutate with Callbacks")
                        .disabled(m_loading)
                        .on_click(cx.listener(|this, _, _, cx| this.do_mutate_with_callbacks(cx))),
                )
                .child(
                    Button::new("pg-mutate-reset")
                        .outline()
                        .label("Reset")
                        .on_click(cx.listener(|this, _, _, cx| this.reset_mutation(cx))),
                ),
        )
        .child(
            v_flex().gap_1().px_4().pb_3()
                .child(
                    h_flex().gap_3().items_center()
                        .child(
                            div()
                                .px_3()
                                .py_1()
                                .rounded(cx.theme().radius_lg)
                                .border_1()
                                .border_color(status_color)
                                .text_sm()
                                .text_color(status_color)
                                .child(m_status.label().to_string()),
                        )
                        .when_some(m_data, |el, d| {
                            el.child(chip(&format!("data: {}", d), cx.theme().background, cx))
                        })
                        .when_some(m_vars, |el, v| {
                            el.child(chip(&format!("vars: {}", v), cx.theme().background, cx))
                        })
                        .when_some(m_error, |el, e| {
                            el.child(
                                div()
                                    .px_3()
                                    .py_1()
                                    .rounded(cx.theme().radius_lg)
                                    .border_1()
                                    .border_color(cx.theme().danger)
                                    .text_sm()
                                    .text_color(cx.theme().danger)
                                    .child(format!("error: {}", e)),
                            )
                        }),
                ),
        )
    }

    fn render_infinite_query(&mut self, cx: &mut Context<Self>) -> Div {

        let page_count = self.infinite_entity.as_ref().map_or(0, |(e, _)| {
            e.read_with(cx, |r, _| r.page_count())
        });
        let inf_status = self.infinite_entity.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let has_next = self.infinite_entity.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.has_next_page())
        });
        let has_prev = self.infinite_entity.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.has_previous_page())
        });
        let loading = inf_status.is_loading();
        let pages: Vec<(usize, PlaygroundPage)> = self.infinite_entity.as_ref()
            .map(|(e, _)| {
                let mut result = Vec::new();
                e.read_with(cx, |r, _| {
                    for (i, page) in r.pages().iter().enumerate() {
                        result.push((i, page.clone()));
                    }
                });
                result
            })
            .unwrap_or_default();

        section_card(
            "Infinite Query",
            "Paginated data with max_pages=3 so eviction is visible. Fetcher generates pages of 5 items each.",
            cx,
        )
        .child(
            h_flex().gap_2().flex_wrap().px_4().py_3()
                .child(
                    Button::new("pg-inf-next")
                        .primary()
                        .label("Load Next Page")
                        .disabled(loading || !has_next)
                        .on_click(cx.listener(|this, _, _, cx| this.load_next_page(cx))),
                )
                .child(
                    Button::new("pg-inf-prev")
                        .outline()
                        .label("Load Previous Page")
                        // Finding 3: Disable when has_prev is false, mirroring
                        // the 'Load Next Page' button's !has_next check.
                        .disabled(loading || !has_prev)
                        .on_click(cx.listener(|this, _, _, cx| this.load_prev_page(cx))),
                )
                .child(
                    Button::new("pg-inf-reset")
                        .outline()
                        .label("Reset")
                        .on_click(cx.listener(|this, _, _, cx| this.reset_infinite(cx))),
                )
                .child(status_badge(inf_status, cx))
                .child(chip(&format!("pages: {}/3 (max)", page_count), cx.theme().background, cx))
                .child(chip(&format!("has_next: {}", has_next), cx.theme().background, cx))
                .child(chip(&format!("has_prev: {}", has_prev), cx.theme().background, cx)),
        )
        .when(!pages.is_empty(), |el| {
            el.child(
                v_flex().gap_2().px_4().pb_3()
                    .children(pages.into_iter().map(|(idx, page)| {
                        div()
                            .px_3()
                            .py_2()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().muted)
                            .child(
                                v_flex().gap_1()
                                    .child(
                                        div().text_xs().font_weight(FontWeight::SEMIBOLD)
                                            .child(format!("Page {} (index {})", page.page_number, idx)),
                                    )
                                    .child(
                                        div().text_xs().text_color(cx.theme().muted_foreground)
                                            .child(page.items.join(", ")),
                                    ),
                            )
                    })),
            )
        })
    }

    fn render_select_transform(&mut self, cx: &mut Context<Self>) -> Div {

        let source_data = self.select_source.as_ref().and_then(|e| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let mapped_data = self.select_mapped.as_ref().and_then(|e| {
            e.read_with(cx, |r, _| r.data())
        });
        let source_status = self.select_source.as_ref().map_or(QueryStatus::Idle, |e| {
            e.read_with(cx, |r, _| r.status())
        });
        let loading = source_status.is_loading();

        section_card(
            "Select Transform",
            "Source query returns Vec<PlaygroundUser>. Transform projects to Vec<String> (names only).",
            cx,
        )
        .child(
            h_flex().gap_2().px_4().py_3()
                .child(
                    Button::new("pg-select-fetch")
                        .primary()
                        .label(if loading { "Fetching..." } else { "Fetch Source" })
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| this.fetch_select(cx))),
                )
                .child(status_badge(source_status, cx)),
        )
        .child(
            h_flex().gap_4().px_4().pb_3()
                .child(
                    v_flex().gap_1()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Source (Vec<User>)"))
                        .child(
                            div()
                                .p_2()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().muted)
                                .text_sm()
                                .child(source_preview(&source_data)),
                        ),
                )
                .child(
                    v_flex().gap_1()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Mapped (Vec<String>)"))
                        .child(
                            div()
                                .p_2()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().muted)
                                .text_sm()
                                .child(mapped_preview(&mapped_data)),
                        ),
                ),
        )
    }

    fn render_imperative_fetch(&mut self, cx: &mut Context<Self>) -> Div {

        let status = self.imperative_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let data = self.imperative_query.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let signal_cancelled = self.imperative_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| {
                r.signal().map(|s| s.is_cancelled()).unwrap_or(false)
            })
        });
        let loading = status.is_loading();

        section_card(
            "Imperative Fetch",
            "Manual refetch with signal. Cancel mid-flight to observe cooperative cancellation.",
            cx,
        )
        .child(
            h_flex().gap_2().flex_wrap().px_4().py_3()
                .child(
                    Button::new("pg-imp-fetch")
                        .primary()
                        .label(if loading { "Fetching..." } else { "Fetch" })
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| this.fetch_imperative(cx))),
                )
                .child(
                    Button::new("pg-imp-cancel")
                        .outline()
                        .label("Cancel mid-flight")
                        .disabled(!loading)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_imperative(cx))),
                )
                .child(
                    Button::new("pg-imp-reset")
                        .outline()
                        .label("Reset")
                        .on_click(cx.listener(|this, _, _, cx| this.reset_imperative(cx))),
                ),
        )
        .child(
            h_flex().gap_3().items_center().px_4().pb_3()
                .child(status_badge(status, cx))
                .when_some(data, |el, d| {
                    el.child(chip(&d, cx.theme().background, cx))
                })
                .child(chip(
                    &format!("signal cancelled: {}", signal_cancelled),
                    cx.theme().background,
                    cx,
                )),
        )
    }

    fn render_activity_log(&self, cx: &mut Context<Self>) -> Div {
        section_card("Activity Log", "Tracks user actions across all sections.", cx)
            .child(
                // Finding 9: Use overflow_y_scroll so users can scroll through
                // all log entries instead of clipping them.
                v_flex().id("activity-log-scroll").gap_0p5().px_4().pb_3().max_h(px(200.)).overflow_y_scroll()
                    .when(self.activity_log.is_empty(), |el| {
                        el.child(
                            div().text_sm().text_color(cx.theme().muted_foreground)
                                .child("No activity yet. Click a button above."),
                        )
                    })
                    .children(self.activity_log.iter().rev().map(|entry| {
                        div()
                            .text_xs()
                            .font_family("monospace")
                            .text_color(cx.theme().muted_foreground)
                            .child(entry.clone())
                    })),
            )
    }
}

// ---------------------------------------------------------------------------
// UI helpers
// ---------------------------------------------------------------------------

fn section_card(title: &str, description: &str, cx: &App) -> Div {
    div()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .overflow_hidden()
        .child(
            div()
                .px_4()
                .py_3()
                .bg(cx.theme().muted)
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    v_flex().gap_1()
                        .child(
                            div().text_base().font_weight(FontWeight::SEMIBOLD).child(title.to_string()),
                        )
                        .child(
                            div().text_xs().text_color(cx.theme().muted_foreground)
                                .child(description.to_string()),
                        ),
                ),
        )
}

fn mini_card(label: &str, cx: &App) -> Div {
    v_flex()
        .gap_2()
        .p_3()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .flex_1()
        .child(
            div().text_sm().font_weight(FontWeight::SEMIBOLD).child(label.to_string()),
        )
}

fn status_badge(status: QueryStatus, cx: &App) -> Div {
    let color = match status {
        QueryStatus::Idle => cx.theme().muted_foreground,
        QueryStatus::LoadingEmpty | QueryStatus::LoadingWithData => cx.theme().info,
        QueryStatus::Success => cx.theme().success,
        QueryStatus::Failure => cx.theme().danger,
        QueryStatus::Cancelled => cx.theme().warning,
    };
    div()
        .px_3()
        .py_1()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(color)
        .text_sm()
        .text_color(color)
        .child(status.label().to_string())
}

fn chip(label: &str, background: Hsla, cx: &App) -> Div {
    div()
        .px_3()
        .py_1()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(background)
        .text_sm()
        .child(label.to_string())
}

/// Finding 12: Render source preview with each entry on its own line instead
/// of joining with `\n` which may not render as visual line breaks in GPUI.
fn source_preview(data: &Option<Vec<PlaygroundUser>>) -> Div {
    match data {
        Some(users) => {
            let el = v_flex().gap_0p5();
            users.iter().fold(el, |el, u| {
                el.child(div().child(format!("{} ({}): {}", u.id, u.name, u.email)))
            })
        }
        None => v_flex().child(div().child("No data")),
    }
}

/// Finding 12: Render mapped preview as a single line (already was fine, but
/// now returns a Div for consistency with source_preview).
fn mapped_preview(data: &Option<Vec<String>>) -> Div {
    match data {
        Some(names) => v_flex().child(div().child(format!("[{}]", names.join(", ")))),
        None => v_flex().child(div().child("No data")),
    }
}
