use gpui::*;

use gpui_query::core::QueryError;
use gpui_query::hook::{
    MutationCallbacks, fetch_next_page_infinite, fetch_previous_page_infinite, fetch_query,
    fetch_query_with_signal, mutate, mutate_with_callbacks,
};

use super::super::{HttpFetchKind, HttpFetchResult, PlaygroundPage, PlaygroundUser, QueryPlaygroundPage};

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

impl QueryPlaygroundPage {
    pub(in super::super) fn fetch_simple(&mut self, cx: &mut Context<Self>) {
        self.ensure_simple_query(cx);
        // Finding 5: Use defensive `if let Some` instead of `unwrap()`.
        let Some((entity, _)) = self.simple_query.as_ref() else {
            return;
        };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        self.log("Simple: fetch triggered");
        fetch_query_with_signal(
            &entity,
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
    }

    pub(in super::super) fn cancel_simple(&mut self, cx: &mut Context<Self>) {
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

    pub(in super::super) fn reset_simple(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.simple_query {
            entity.update(cx, |r, _| r.reset());
            self.log("Simple: reset");
            cx.notify();
        }
    }

    pub(in super::super) fn fetch_nocache(&mut self, cx: &mut Context<Self>) {
        self.ensure_nocache_query(cx);
        let Some((entity, _)) = self.nocache_query.as_ref() else {
            return;
        };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        fetch_query_with_signal(
            &entity,
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
    }

    pub(in super::super) fn fetch_ttl(&mut self, cx: &mut Context<Self>) {
        self.ensure_ttl_query(cx);
        let Some((entity, _)) = self.ttl_query.as_ref() else {
            return;
        };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        fetch_query_with_signal(
            &entity,
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
    }

    pub(in super::super) fn fetch_swr(&mut self, cx: &mut Context<Self>) {
        self.ensure_swr_query(cx);
        let Some((entity, _)) = self.swr_query.as_ref() else {
            return;
        };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        fetch_query_with_signal(
            &entity,
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
    }

    pub(in super::super) fn spam_latest_wins(&mut self, cx: &mut Context<Self>) {
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
            fetch_query_with_signal(
                &e,
                move |_signal| {
                    let l = label.clone();
                    let exec = exec.clone();
                    async move {
                        exec.timer(std::time::Duration::from_secs(2)).await;
                        Ok(l)
                    }
                },
                cx,
            );
        }
    }

    pub(in super::super) fn spam_ignore(&mut self, cx: &mut Context<Self>) {
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
            fetch_query_with_signal(
                &e,
                move |_signal| {
                    let l = label.clone();
                    let exec = exec.clone();
                    async move {
                        exec.timer(std::time::Duration::from_secs(2)).await;
                        Ok(l)
                    }
                },
                cx,
            );
        }
    }

    pub(in super::super) fn trigger_failing_fetch(&mut self, cx: &mut Context<Self>) {
        self.ensure_retry_query(cx);
        let Some((entity, _)) = self.retry_query.as_ref() else {
            return;
        };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        self.log("Retry: triggered failing fetch (3 retries, 400ms backoff)");
        // NOTE: use `fetch_query` (retry-aware, Fn fetcher), NOT
        // `fetch_query_with_signal` — the latter is FnOnce/single-shot and the
        // crate explicitly skips retries for it, so the RetryPolicy would never
        // fire and the count would be stuck at 1.
        fetch_query(
            &entity,
            move || {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_millis(300)).await;
                    Err(QueryError::response("simulated failure"))
                }
            },
            cx,
        );
    }

    pub(in super::super) fn do_mutate(&mut self, cx: &mut Context<Self>) {
        self.ensure_mutation(cx);
        let Some((entity, _)) = self.mutation_entity.as_ref() else {
            return;
        };
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
        mutate(
            &entity,
            vars,
            move |v| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_secs(1)).await;
                    Ok(format!("result for: {}", v))
                }
            },
            cx,
        );
    }

    pub(in super::super) fn do_mutate_with_callbacks(&mut self, cx: &mut Context<Self>) {
        self.ensure_mutation(cx);
        let Some((entity, _)) = self.mutation_entity.as_ref() else {
            return;
        };
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
                log_for_success
                    .lock()
                    .unwrap()
                    .push(format!("on_success: {}", data));
            })
            .on_error(move |err: &QueryError| {
                log_for_error
                    .lock()
                    .unwrap()
                    .push(format!("on_error: {}", err));
            });

        mutate_with_callbacks(
            &entity,
            vars,
            move |v| {
                let exec = exec.clone();
                async move {
                    exec.timer(std::time::Duration::from_secs(1)).await;
                    Ok(format!("callback result for: {}", v))
                }
            },
            callbacks,
            cx,
        );

        self.log("Mutation: callbacks registered (on_success, on_error)");
    }

    pub(in super::super) fn reset_mutation(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.mutation_entity {
            entity.update(cx, |r, _| r.reset());
            self.log("Mutation: reset");
            cx.notify();
        }
    }

    pub(in super::super) fn load_next_page(&mut self, cx: &mut Context<Self>) {
        self.ensure_infinite(cx);
        let Some((entity, _)) = self.infinite_entity.as_ref() else {
            return;
        };
        let entity = entity.clone();
        // Finding 4: Derive page count from entity state instead of tracking
        // a separate `next_page` counter that goes out of sync after resets.
        let current_pages = entity.read_with(cx, |r, _| r.page_count());
        let exec = cx.background_executor().clone();
        self.log(format!(
            "Infinite: load next page (current pages: {})",
            current_pages
        ));
        // The crate's `has_next_page` is only a fetch gate and goes stale (it's
        // only updated by forward fetches); force it true so `begin_fetch_next`
        // proceeds. The UI derives the real "can next" state from the loaded
        // page range in `render_infinite_query`.
        entity.update(cx, |r, _| r.set_has_next_page(true));
        fetch_next_page_infinite(
            &entity,
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
    }

    pub(in super::super) fn load_prev_page(&mut self, cx: &mut Context<Self>) {
        self.ensure_infinite(cx);
        let Some((entity, _)) = self.infinite_entity.as_ref() else {
            return;
        };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        self.log("Infinite: load previous page");
        // Force the fetch gate open — see the matching note in `load_next_page`.
        entity.update(cx, |r, _| r.set_has_previous_page(true));
        fetch_previous_page_infinite(
            &entity,
            move |first_page: Option<&PlaygroundPage>| {
                let exec = exec.clone();
                let page_num = first_page
                    .map(|p| p.page_number.saturating_sub(1))
                    .unwrap_or(5);
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
                        page_num > 0,
                    ))
                }
            },
            cx,
        );
    }

    pub(in super::super) fn reset_infinite(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.infinite_entity {
            entity.update(cx, |r, _| r.reset());
            // Finding 4: No separate `next_page` counter to reset.
            self.log("Infinite: reset");
            cx.notify();
        }
    }

    pub(in super::super) fn fetch_select(&mut self, cx: &mut Context<Self>) {
        self.ensure_select(cx);
        let source = self.select_source.clone();
        if let Some(source) = source {
            let exec = cx.background_executor().clone();
            fetch_query_with_signal(
                &source,
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
            self.log("Select: source fetch triggered");
        }
    }

    pub(in super::super) fn fetch_imperative(&mut self, cx: &mut Context<Self>) {
        self.ensure_imperative(cx);
        let Some((entity, _)) = self.imperative_query.as_ref() else {
            return;
        };
        let entity = entity.clone();
        let exec = cx.background_executor().clone();
        self.log("Imperative: fetch triggered");
        fetch_query_with_signal(
            &entity,
            move |signal| {
                let exec = exec.clone();
                async move {
                    // Cooperative cancellation: poll the signal between short
                    // timer slices (40 × 50ms = 2s total) so a mid-flight cancel
                    // is observed within ~50ms instead of running the full 2s.
                    for _ in 0..40 {
                        if signal.is_cancelled() {
                            return Err(QueryError::cancelled("cancelled mid-flight"));
                        }
                        exec.timer(std::time::Duration::from_millis(50)).await;
                    }
                    Ok("imperative result".into())
                }
            },
            cx,
        );
    }

    pub(in super::super) fn cancel_imperative(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.imperative_query {
            // Mark the active request as cancelled: this clears
            // `active_request_id` (so the in-flight result is discarded by
            // `accept_current_request`), transitions status to `Cancelled`,
            // and cancels the cooperative signal. Merely flipping the signal
            // flag — as this previously did — left the request active, so the
            // late result still landed as Success and nothing re-rendered.
            entity.update(cx, |r, _| {
                r.cancel(QueryError::cancelled("cancelled mid-flight"));
            });
            self.log("Imperative: cancelled mid-flight");
            cx.notify();
        }
    }

    pub(in super::super) fn reset_imperative(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.imperative_query {
            entity.update(cx, |r, _| r.reset());
            self.log("Imperative: reset");
            cx.notify();
        }
    }

    // -----------------------------------------------------------------------
    // HTTP Fetching — real network requests via reqwest over the tokio runtime.
    // -----------------------------------------------------------------------

    pub(in super::super) fn fetch_http(&mut self, kind: HttpFetchKind, cx: &mut Context<Self>) {
        self.ensure_http_query(cx);
        let Some((entity, _)) = self.http_query.as_ref() else {
            return;
        };
        let entity = entity.clone();

        // reqwest must run on tokio; clone the shared client + runtime out of the
        // global (releasing the cx borrow) before entering the fetch closure.
        let (client, runtime) = match crate::services::tokio_runtime::runtime_and_client(cx) {
            Some((runtime, client)) => (client, runtime),
            None => {
                self.log(format!(
                    "HTTP: tokio runtime unavailable — cannot {} {}",
                    kind.method(),
                    kind.url()
                ));
                return;
            }
        };

        self.log(format!(
            "HTTP: {} {} → {}",
            kind.label(),
            kind.method(),
            kind.url()
        ));

        fetch_query_with_signal(
            &entity,
            move |signal| {
                let client = client.clone();
                let runtime = runtime.clone();
                async move {
                    if signal.is_cancelled() {
                        return Err(QueryError::cancelled("cancelled before send"));
                    }
                    run_http(&client, &runtime, kind).await
                }
            },
            cx,
        );
    }

    pub(in super::super) fn reset_http(&mut self, cx: &mut Context<Self>) {
        if let Some((entity, _)) = &self.http_query {
            entity.update(cx, |r, _| r.reset());
            self.log("HTTP: reset");
            cx.notify();
        }
    }
}

// ---------------------------------------------------------------------------
// Shared HTTP executor (free functions)
//
// reqwest cannot run on gpui's (non-tokio) executor, so each request is
// `runtime.spawn`-ed onto the tokio runtime and the JoinHandle is awaited from
// the fetch closure. Shared by the lazy initial fetch (GET JSON, in `init`) and
// `fetch_http`.
// ---------------------------------------------------------------------------

pub(super) async fn run_http(
    client: &reqwest::Client,
    runtime: &std::sync::Arc<tokio::runtime::Runtime>,
    kind: HttpFetchKind,
) -> Result<HttpFetchResult, QueryError> {
    let started = std::time::Instant::now();
    let url = kind.url().to_string();
    let join = {
        let client = client.clone();
        let url = url.clone();
        runtime.spawn(async move {
            let resp = build_request(&client, kind, &url)
                .send()
                .await
                .map_err(|e| QueryError::response(format!("send: {e}")))?;
            let status = resp.status().as_u16();
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let raw = resp
                .text()
                .await
                .map_err(|e| QueryError::response(format!("body: {e}")))?;
            Ok::<_, QueryError>((status, content_type, raw))
        })
    };
    let (status, content_type, raw) = join
        .await
        .map_err(|e| QueryError::response(format!("join: {e}")))??;

    // Pretty-print JSON bodies for readability; truncate long bodies for display.
    let is_json = matches!(kind, HttpFetchKind::GetJson | HttpFetchKind::PostJson);
    let body = if is_json {
        serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or(raw)
    } else {
        raw
    };
    let body = if body.chars().count() > 1500 {
        format!("{}…", body.chars().take(1500).collect::<String>())
    } else {
        body
    };

    Ok(HttpFetchResult {
        method: kind.method().into(),
        label: kind.label().into(),
        url,
        status,
        content_type,
        body,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

fn build_request(
    client: &reqwest::Client,
    kind: HttpFetchKind,
    url: &str,
) -> reqwest::RequestBuilder {
    match kind {
        HttpFetchKind::PostJson => {
            let payload = serde_json::json!({
                "name": "gpui-starter",
                "section": "query_playground",
                "nested": { "source": "httpbin" },
            });
            client.post(url).json(&payload)
        }
        _ => client.get(url).header("accept", kind.accept_header()),
    }
}
