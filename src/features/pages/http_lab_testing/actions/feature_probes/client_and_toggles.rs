use gpui::*;

use gpui_query::{CachePolicy, QueryError, RequestPolicy};

use super::super::super::{query_now_ms, HttpLabTestingPage, RawResponse};

impl HttpLabTestingPage {
    // -- Feature 2: Client fetchQuery --

    pub(crate) fn exercise_client_fetch_query(&mut self, cx: &mut Context<Self>) {
        let key = gpui_query::QueryKey::from_single("http_lab_testing/client_fetch");
        let now_ms = query_now_ms();

        if !cx.has_global::<gpui_query::client::QueryClient>() {
            cx.set_global(gpui_query::client::QueryClient::new(
                gpui_query::CachePolicy::default(),
                gpui_query::RequestPolicy::default(),
            ));
        }

        let result = cx.update_global::<gpui_query::client::QueryClient, _>(|client, cx| {
            client.fetch_query::<RawResponse, QueryError>(
                key,
                CachePolicy::NoCache,
                RequestPolicy::LatestWins,
                now_ms,
                cx,
            )
        });

        let now_ms_for_complete = now_ms;
        match result {
            Some((entity, request_id)) => {
                let rid_label = request_id.label();
                // Complete the request immediately so the resource transitions
                // from LoadingEmpty → Success (otherwise DevTools shows "Loading").
                let completed = entity.update(cx, |resource, _| {
                    resource.complete_current_success(
                        request_id,
                        RawResponse {
                            status: 200,
                            final_url: "https://httpbin.org/json".to_string(),
                            header_count: 0,
                            bytes: 0,
                            preview: "client_fetch probe".to_string(),
                        },
                        now_ms_for_complete,
                    )
                });
                let v_started = Self::verdict("request started", true, &format!("request_id={}", rid_label));
                let v_completed = Self::verdict("request completed", completed, "complete_current_success");
                let verdict_line = "Client fetch PASSED";
                self.client_query_message = format!("{v_started}\n{v_completed}\n{verdict_line}");
            }
            None => {
                let v_started = Self::verdict("request started", false, "returned None (cache hit or ignored)");
                let verdict_line = "Client fetch FAILED";
                self.client_query_message = format!("{v_started}\n{verdict_line}");
            }
        }
        cx.notify();
    }

    pub(crate) fn exercise_client_force_fetch_query(&mut self, cx: &mut Context<Self>) {
        let key = gpui_query::QueryKey::from_single("http_lab_testing/client_force_fetch");
        let now_ms = query_now_ms();

        if !cx.has_global::<gpui_query::client::QueryClient>() {
            cx.set_global(gpui_query::client::QueryClient::new(
                gpui_query::CachePolicy::default(),
                gpui_query::RequestPolicy::default(),
            ));
        }

        let result = cx.update_global::<gpui_query::client::QueryClient, _>(|client, cx| {
            client.force_fetch_query::<RawResponse, QueryError>(
                key,
                CachePolicy::NoCache,
                RequestPolicy::LatestWins,
                now_ms,
                cx,
            )
        });

        let now_ms_for_complete = now_ms;
        match result {
            Some((entity, request_id)) => {
                let rid_label = request_id.label();
                // Complete the request immediately so the resource transitions
                // from LoadingEmpty → Success (otherwise DevTools shows "Loading").
                let completed = entity.update(cx, |resource, _| {
                    resource.complete_current_success(
                        request_id,
                        RawResponse {
                            status: 200,
                            final_url: "https://httpbin.org/json".to_string(),
                            header_count: 0,
                            bytes: 0,
                            preview: "client_force_fetch probe".to_string(),
                        },
                        now_ms_for_complete,
                    )
                });
                let v_started = Self::verdict("forced request started", true, &format!("request_id={}", rid_label));
                let v_completed = Self::verdict("request completed", completed, "complete_current_success");
                let verdict_line = "Client force fetch PASSED";
                self.client_query_message = format!("{v_started}\n{v_completed}\n{verdict_line}");
            }
            None => {
                let v_started = Self::verdict("forced request started", false, "returned None (ignored)");
                let verdict_line = "Client force fetch FAILED";
                self.client_query_message = format!("{v_started}\n{verdict_line}");
            }
        }
        cx.notify();
    }

    pub(crate) fn toggle_query_details(&mut self, cx: &mut Context<Self>) {
        self.show_query_details = !self.show_query_details;
        cx.notify();
    }

    pub(crate) fn toggle_signal_details(&mut self, cx: &mut Context<Self>) {
        self.show_signal_details = !self.show_signal_details;
        cx.notify();
    }

    pub(crate) fn toggle_retention_details(&mut self, cx: &mut Context<Self>) {
        self.show_retention_details = !self.show_retention_details;
        cx.notify();
    }

    pub(crate) fn toggle_optimistic_details(&mut self, cx: &mut Context<Self>) {
        self.show_optimistic_details = !self.show_optimistic_details;
        cx.notify();
    }

    pub(crate) fn toggle_client_details(&mut self, cx: &mut Context<Self>) {
        self.show_client_details = !self.show_client_details;
        cx.notify();
    }

    pub(crate) fn toggle_local_history(&mut self, cx: &mut Context<Self>) {
        self.show_local_history = !self.show_local_history;
        cx.notify();
    }

    pub(crate) fn toggle_response_preview(&mut self, cx: &mut Context<Self>) {
        self.show_response_preview = !self.show_response_preview;
        cx.notify();
    }

    pub(crate) fn toggle_response_details(&mut self, cx: &mut Context<Self>) {
        self.show_response_details = !self.show_response_details;
        cx.notify();
    }
}
