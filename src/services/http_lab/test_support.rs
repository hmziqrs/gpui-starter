use super::{
    client::HTTP_LAB_BASE,
    transitions::{apply_result_to_state, begin_action},
    *,
};
use gpui_query_legacy::{QueryError, QueryResource};

pub(super) fn exchange(label: &str, status: u16, error: Option<&str>) -> HttpExchange {
    HttpExchange {
        label: label.to_string(),
        request: HttpRequestSnapshot {
            method: "GET".to_string(),
            url: format!("{HTTP_LAB_BASE}/test"),
            request_body_kind: HttpRequestBodyKind::None,
            request_body_preview: String::new(),
        },
        response: Some(HttpResponseSnapshot {
            status,
            status_text: "test".to_string(),
            final_url: format!("{HTTP_LAB_BASE}/test"),
            elapsed_ms: 1,
            headers: Vec::new(),
            body_kind: HttpBodyKind::Text,
            body_preview: String::new(),
            parsed_json: None,
            parsed_xml_preview: None,
        }),
        error: error.map(str::to_string),
    }
}

pub(super) fn seed_response(
    state: &mut HttpLabState,
    action: HttpLabAction,
    response: HttpExchange,
    now_ms: u128,
) {
    let request = begin_action(state, action, now_ms.saturating_sub(1)).expect("seed request");
    apply_result_to_state(state, action, request, Ok(vec![(action, response)]), now_ms);
}

pub(super) fn error_message(resource: &QueryResource<HttpExchange>) -> Option<&str> {
    resource.error().map(QueryError::message)
}
