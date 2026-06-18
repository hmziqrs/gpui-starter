use gpui::*;
use gpui_query_legacy::{QueryBeginResult, QueryError, QueryFetchMode};

use super::super::super::{HttpLabTestingPage, fake_response, query_now_ms};

impl HttpLabTestingPage {
    // -- Feature 1: Cancel Signal --

    pub(crate) fn exercise_query_signal(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();
        // Reset to clear any previous state.
        self.query_signal_resource.reset();
        let result = self.query_signal_resource.begin_request(
            &mut self.query_signal_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );

        let QueryBeginResult::Started { request_id: _, .. } = result else {
            self.query_signal_message = format!("Signal setup did not start: {result:?}");
            cx.notify();
            return;
        };

        // Clone the signal before cancelling.
        let signal = self.query_signal_resource.signal().cloned();
        let signal_present = signal.is_some();
        let before_cancel = signal.as_ref().map(|s| s.is_cancelled());

        // Cancel the resource — this should propagate to the signal.
        self.query_signal_resource
            .cancel(QueryError::cancelled("signal test"));
        let after_cancel = signal.as_ref().map(|s| s.is_cancelled());

        let v_signal = Self::verdict(
            "signal present",
            signal_present,
            &format!("signal_present={signal_present}"),
        );
        let before_ok = before_cancel == Some(false);
        let v_before = Self::verdict(
            "signal active before cancel",
            before_ok,
            &format!("before_cancel={:?}", before_cancel),
        );
        let after_ok = after_cancel == Some(true);
        let v_after = Self::verdict(
            "signal cancelled after resource cancel",
            after_ok,
            &format!("after_cancel={:?}", after_cancel),
        );
        let all_passed = signal_present && before_ok && after_ok;
        let verdict_line = if all_passed {
            "Cancel signal probe PASSED"
        } else {
            "Cancel signal probe FAILED"
        };
        self.query_signal_message = format!("{v_signal}\n{v_before}\n{v_after}\n{verdict_line}");
        cx.notify();
    }

    // -- Feature 3: Placeholder / Previous Data --

    pub(crate) fn exercise_query_placeholder_data(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();

        // Step 1: Seed the resource with real data.
        self.query_placeholder_resource.reset();
        let first = self.query_placeholder_resource.begin_request(
            &mut self.query_placeholder_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        let QueryBeginResult::Started {
            request_id: first_id,
            ..
        } = first
        else {
            self.query_placeholder_message = format!("Placeholder setup did not start: {first:?}");
            cx.notify();
            return;
        };
        self.query_placeholder_resource.complete_current_success(
            first_id,
            fake_response("original"),
            now_ms + 1,
        );

        // Step 2: Set placeholder data, then reset (clears data).
        self.query_placeholder_resource
            .set_placeholder_data(Some(fake_response("placeholder")));

        // Step 3: Reset clears data but NOT placeholder (actually reset DOES clear placeholder).
        // So set placeholder AFTER reset.
        self.query_placeholder_resource.reset();
        self.query_placeholder_resource
            .set_placeholder_data(Some(fake_response("placeholder")));

        // Step 4: Begin new request — during loading, display_data returns placeholder.
        let second = self.query_placeholder_resource.begin_request(
            &mut self.query_placeholder_sequencer,
            now_ms + 10,
            QueryFetchMode::Normal,
        );
        let loading_display = self
            .query_placeholder_resource
            .display_data()
            .map(|r| r.preview.clone());

        // Step 5: Complete with real data.
        if let QueryBeginResult::Started {
            request_id: second_id,
            ..
        } = second
        {
            self.query_placeholder_resource.complete_current_success(
                second_id,
                fake_response("real"),
                now_ms + 11,
            );
        }

        let final_display = self
            .query_placeholder_resource
            .display_data()
            .map(|r| r.preview.clone());
        let previous = self
            .query_placeholder_resource
            .previous_data()
            .map(|r| r.preview.clone());

        let loading_ok = loading_display.as_deref() == Some("placeholder");
        let v_loading = Self::verdict(
            "placeholder shown during loading",
            loading_ok,
            &format!("loading_display={loading_display:?}"),
        );
        let final_ok = final_display.as_deref() == Some("real");
        let v_final = Self::verdict(
            "real data after completion",
            final_ok,
            &format!("final_display={final_display:?}"),
        );
        let previous_ok = previous.as_deref() == Some("original");
        let v_previous = Self::verdict(
            "previous tracked as original",
            previous_ok,
            &format!("previous={previous:?}"),
        );
        let all_passed = loading_ok && final_ok && previous_ok;
        let verdict_line = if all_passed {
            "Placeholder data probe PASSED"
        } else {
            "Placeholder data probe FAILED"
        };
        self.query_placeholder_message =
            format!("{v_loading}\n{v_final}\n{v_previous}\n{verdict_line}");
        cx.notify();
    }

    pub(crate) fn exercise_query_previous_data(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();

        // Seed "first" then "second".
        self.query_placeholder_resource.reset();
        let first = self.query_placeholder_resource.begin_request(
            &mut self.query_placeholder_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = first {
            self.query_placeholder_resource.complete_current_success(
                request_id,
                fake_response("first"),
                now_ms + 1,
            );
        }

        let second = self.query_placeholder_resource.begin_request(
            &mut self.query_placeholder_sequencer,
            now_ms + 10,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = second {
            self.query_placeholder_resource.complete_current_success(
                request_id,
                fake_response("second"),
                now_ms + 11,
            );
        }

        let data = self
            .query_placeholder_resource
            .data()
            .map(|r| r.preview.clone());
        let previous = self
            .query_placeholder_resource
            .previous_data()
            .map(|r| r.preview.clone());

        let data_ok = data.as_deref() == Some("second");
        let v_data = Self::verdict(
            "current data is 'second'",
            data_ok,
            &format!("data={data:?}"),
        );
        let previous_ok = previous.as_deref() == Some("first");
        let v_previous = Self::verdict(
            "previous data is 'first'",
            previous_ok,
            &format!("previous={previous:?}"),
        );
        let all_passed = data_ok && previous_ok;
        let verdict_line = if all_passed {
            "Previous data probe PASSED"
        } else {
            "Previous data probe FAILED"
        };
        self.query_placeholder_message = format!("{v_data}\n{v_previous}\n{verdict_line}");
        cx.notify();
    }

    pub(crate) fn exercise_query_rollback(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();

        // Seed data, overwrite, then rollback.
        self.query_placeholder_resource.reset();
        let first = self.query_placeholder_resource.begin_request(
            &mut self.query_placeholder_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = first {
            self.query_placeholder_resource.complete_current_success(
                request_id,
                fake_response("original"),
                now_ms + 1,
            );
        }

        // Overwrite with new data.
        self.query_placeholder_resource
            .set_data(fake_response("overwritten"));

        // Rollback to previous.
        let rolled_back = self.query_placeholder_resource.rollback_to_previous();

        let data = self
            .query_placeholder_resource
            .data()
            .map(|r| r.preview.clone());

        let data_ok = data.as_deref() == Some("original");
        let v_rollback = Self::verdict(
            "rollback succeeded",
            rolled_back,
            &format!("rolled_back={rolled_back}"),
        );
        let v_data = Self::verdict(
            "data restored to 'original'",
            data_ok,
            &format!("data={data:?}"),
        );
        let all_passed = rolled_back && data_ok;
        let verdict_line = if all_passed {
            "Rollback probe PASSED"
        } else {
            "Rollback probe FAILED"
        };
        self.query_placeholder_message = format!("{v_rollback}\n{v_data}\n{verdict_line}");
        cx.notify();
    }

    // -- Feature 4: Optimistic Updates --

    pub(crate) fn exercise_query_optimistic_set(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();

        // Seed original data.
        self.query_optimistic_resource.reset();
        let first = self.query_optimistic_resource.begin_request(
            &mut self.query_optimistic_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = first {
            self.query_optimistic_resource.complete_current_success(
                request_id,
                fake_response("original"),
                now_ms + 1,
            );
        }

        // Optimistic update.
        self.query_optimistic_resource
            .set_data(fake_response("optimistic"));

        let data = self
            .query_optimistic_resource
            .data()
            .map(|r| r.preview.clone());
        let previous = self
            .query_optimistic_resource
            .previous_data()
            .map(|r| r.preview.clone());
        let status = self.query_optimistic_resource.status().label().to_string();

        let data_ok = data.as_deref() == Some("optimistic");
        let previous_ok = previous.as_deref() == Some("original");
        let status_ok = status == "Success";
        let v_data = Self::verdict("data is 'optimistic'", data_ok, &format!("data={data:?}"));
        let v_previous = Self::verdict(
            "previous is 'original'",
            previous_ok,
            &format!("previous={previous:?}"),
        );
        let v_status = Self::verdict("status is Success", status_ok, &format!("status={status}"));
        let all_passed = data_ok && previous_ok && status_ok;
        let verdict_line = if all_passed {
            "Optimistic set probe PASSED"
        } else {
            "Optimistic set probe FAILED"
        };
        self.query_optimistic_message =
            format!("{v_data}\n{v_previous}\n{v_status}\n{verdict_line}");
        cx.notify();
    }

    pub(crate) fn exercise_query_optimistic_rollback(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();

        // Seed original data.
        self.query_optimistic_resource.reset();
        let first = self.query_optimistic_resource.begin_request(
            &mut self.query_optimistic_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = first {
            self.query_optimistic_resource.complete_current_success(
                request_id,
                fake_response("original"),
                now_ms + 1,
            );
        }

        // Optimistic update then rollback.
        self.query_optimistic_resource
            .set_data(fake_response("optimistic"));
        let rolled_back = self.query_optimistic_resource.rollback_to_previous();

        let data = self
            .query_optimistic_resource
            .data()
            .map(|r| r.preview.clone());
        let data_ok = data.as_deref() == Some("original");
        let v_rollback = Self::verdict(
            "rollback succeeded",
            rolled_back,
            &format!("rolled_back={rolled_back}"),
        );
        let v_data = Self::verdict(
            "data restored to 'original'",
            data_ok,
            &format!("data={data:?}"),
        );
        let all_passed = rolled_back && data_ok;
        let verdict_line = if all_passed {
            "Optimistic rollback probe PASSED"
        } else {
            "Optimistic rollback probe FAILED"
        };
        self.query_optimistic_message = format!("{v_rollback}\n{v_data}\n{verdict_line}");
        cx.notify();
    }

    pub(crate) fn exercise_query_optimistic_flow(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();

        // Seed original.
        self.query_optimistic_resource.reset();
        let first = self.query_optimistic_resource.begin_request(
            &mut self.query_optimistic_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = first {
            self.query_optimistic_resource.complete_current_success(
                request_id,
                fake_response("original"),
                now_ms + 1,
            );
        }

        // Optimistic update.
        self.query_optimistic_resource
            .set_data(fake_response("optimistic"));

        // Simulate mutation success — begin request and complete with server data.
        let mutation = self.query_optimistic_resource.begin_request(
            &mut self.query_optimistic_sequencer,
            now_ms + 10,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = mutation {
            self.query_optimistic_resource.complete_current_success(
                request_id,
                fake_response("server confirmed"),
                now_ms + 11,
            );
        }

        let data = self
            .query_optimistic_resource
            .data()
            .map(|r| r.preview.clone());
        let previous = self
            .query_optimistic_resource
            .previous_data()
            .map(|r| r.preview.clone());

        let data_ok = data.as_deref() == Some("server confirmed");
        let previous_ok = previous.as_deref() == Some("optimistic");
        let v_data = Self::verdict(
            "data is 'server confirmed'",
            data_ok,
            &format!("data={data:?}"),
        );
        let v_previous = Self::verdict(
            "previous is 'optimistic'",
            previous_ok,
            &format!("previous={previous:?}"),
        );
        let all_passed = data_ok && previous_ok;
        let verdict_line = if all_passed {
            "Optimistic flow probe PASSED"
        } else {
            "Optimistic flow probe FAILED"
        };
        self.query_optimistic_message = format!("{v_data}\n{v_previous}\n{verdict_line}");
        cx.notify();
    }
}
