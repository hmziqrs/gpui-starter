use std::collections::BTreeMap;

use gpui::Global;
use serde::{Deserialize, Serialize};

use gpui_query::{QueryResource, QuerySignal, RequestId, RequestSequencer};

use crate::services::http_lab::types::{HttpCookieSnapshot, HttpExchange, HttpLabAction};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpLabState {
    pub selected_action: HttpLabAction,
    pub(super) resources: BTreeMap<HttpLabAction, QueryResource<HttpExchange>>,
    pub(super) request_sequencer: RequestSequencer,
    pub history: Vec<HttpExchange>,
    pub transition_log: Vec<String>,
    pub cookies: Option<HttpCookieSnapshot>,
}

impl Default for HttpLabState {
    fn default() -> Self {
        let mut resources = BTreeMap::new();
        for action in HttpLabAction::all() {
            resources.insert(*action, resource_for_action(*action));
        }

        Self {
            selected_action: HttpLabAction::GetJson,
            resources,
            request_sequencer: RequestSequencer::new(),
            history: Vec::new(),
            transition_log: vec!["Idle".to_string()],
            cookies: None,
        }
    }
}

impl HttpLabState {
    pub fn resource(&self, action: HttpLabAction) -> &QueryResource<HttpExchange> {
        self.resources
            .get(&action)
            .expect("all http lab actions must have resources")
    }

    pub fn selected_resource(&self) -> &QueryResource<HttpExchange> {
        self.resource(self.selected_action)
    }

    pub fn active_count(&self) -> usize {
        self.resources
            .values()
            .filter(|resource| resource.active_request_id().is_some())
            .count()
    }

    // -- Data retention accessors (placeholder, previous, display) --

    /// Returns the data to display for the given action's resource.
    /// Falls back to placeholder data when no real data is present.
    pub fn display_resource(&self, action: HttpLabAction) -> Option<&HttpExchange> {
        self.resource(action).display_data()
    }

    /// Returns the previously held data for the given action's resource.
    /// Set automatically when data is overwritten via success or `set_action_data`.
    pub fn previous_resource_data(&self, action: HttpLabAction) -> Option<&HttpExchange> {
        self.resource(action).previous_data()
    }

    /// Sets placeholder data for the given action's resource.
    /// Shown by `display_resource` when no real data is present.
    pub fn set_placeholder_for_action(
        &mut self,
        action: HttpLabAction,
        data: Option<HttpExchange>,
    ) {
        if let Some(resource) = self.resources.get_mut(&action) {
            resource.set_placeholder_data(data);
        }
    }

    // -- Optimistic update methods --

    /// Optimistically sets data on the given action's resource without
    /// changing status or completing a request. The current data is stored
    /// in `previous_data` for rollback via `rollback_action_data`.
    pub fn set_action_data(&mut self, action: HttpLabAction, data: HttpExchange) {
        if let Some(resource) = self.resources.get_mut(&action) {
            resource.set_data(data);
        }
    }

    /// Optimistically clears data on the given action's resource.
    /// The current data is stored in `previous_data` for rollback.
    pub fn clear_action_data(&mut self, action: HttpLabAction) {
        if let Some(resource) = self.resources.get_mut(&action) {
            resource.clear_data();
        }
    }

    /// Rolls back to the previously held data for the given action's resource.
    /// Returns `true` if rollback succeeded (previous data existed).
    pub fn rollback_action_data(&mut self, action: HttpLabAction) -> bool {
        if let Some(resource) = self.resources.get_mut(&action) {
            resource.rollback_to_previous()
        } else {
            false
        }
    }

    /// Returns a clone of the cancellation signal for the given action's resource.
    /// The signal is created on `begin_request` and cancelled on `cancel`.
    pub fn signal_for_action(&self, action: HttpLabAction) -> Option<QuerySignal> {
        self.resource(action).signal().cloned()
    }

    pub(super) fn reset_for_user(&mut self) -> ResetRequests {
        let cancelled_requests = self
            .resources
            .values()
            .filter_map(|resource| resource.active_request_id())
            .collect::<Vec<_>>();
        let mut request_sequencer = self.request_sequencer.clone();
        request_sequencer.advance_scope();
        *self = Self::default();
        self.request_sequencer = request_sequencer;
        ResetRequests {
            request_ids: cancelled_requests,
        }
    }
}

impl Global for HttpLabState {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ResetRequests {
    pub(super) request_ids: Vec<RequestId>,
}

fn resource_for_action(action: HttpLabAction) -> QueryResource<HttpExchange> {
    let mut resource = QueryResource::new(
        action.query_key(),
        action.cache_policy(),
        action.request_policy(),
    );
    resource.set_retry_policy(action.retry_policy());
    resource
}
