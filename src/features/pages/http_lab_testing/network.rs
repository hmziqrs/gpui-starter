use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::services::http_lab::HttpLabAction;

use super::{LOG, PREVIEW_LIMIT, RawResponse, TEST_URL};

pub(crate) async fn raw_reqwest_get(
    client: reqwest::Client,
    url: String,
    cancellation: CancellationToken,
    operation_id: u64,
) -> Result<RawResponse, String> {
    tracing::info!(
        target: LOG,
        operation_id,
        url,
        "HTTP Lab Testing raw request build started"
    );

    let request = client
        .get(&url)
        .header("accept", "application/json")
        .header("x-gpui-http-lab-testing", operation_id.to_string());

    tracing::info!(
        target: LOG,
        operation_id,
        "HTTP Lab Testing raw request send started"
    );

    let send_started = Instant::now();
    let mut response = tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
            tracing::info!(
                target: LOG,
                operation_id,
                "HTTP Lab Testing raw request cancelled before response"
            );
            return Err("cancelled".to_string());
        }
        result = request.send() => result.map_err(|err| err.to_string())?,
    };

    let send_elapsed_ms = send_started.elapsed().as_millis();
    let status = response.status().as_u16();
    let final_url = response.url().to_string();
    let header_count = response.headers().len();

    tracing::info!(
        target: LOG,
        operation_id,
        status,
        final_url,
        header_count,
        send_elapsed_ms,
        "HTTP Lab Testing raw request send completed"
    );

    let mut bytes = Vec::new();
    let body_started = Instant::now();
    loop {
        if bytes.len() >= PREVIEW_LIMIT {
            break;
        }

        let chunk = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                tracing::info!(
                    target: LOG,
                    operation_id,
                    bytes = bytes.len(),
                    "HTTP Lab Testing raw body cancelled"
                );
                return Err("cancelled".to_string());
            }
            result = response.chunk() => result.map_err(|err| err.to_string())?,
        };

        let Some(chunk) = chunk else {
            break;
        };

        let remaining = PREVIEW_LIMIT - bytes.len();
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }

    let body_elapsed_ms = body_started.elapsed().as_millis();
    let preview = String::from_utf8_lossy(&bytes).to_string();

    tracing::info!(
        target: LOG,
        operation_id,
        bytes = bytes.len(),
        body_elapsed_ms,
        "HTTP Lab Testing raw body preview completed"
    );

    Ok(RawResponse {
        status,
        final_url,
        header_count,
        bytes: bytes.len(),
        preview,
    })
}

pub(crate) async fn run_local_lab_action(
    client: reqwest::Client,
    action: HttpLabAction,
    cancellation: CancellationToken,
    operation_id: u64,
) -> Result<Vec<(HttpLabAction, RawResponse)>, String> {
    if action == HttpLabAction::FullFlow {
        let mut exchanges = Vec::new();
        for target_action in [
            HttpLabAction::GetJson,
            HttpLabAction::PostJson,
            HttpLabAction::Cookies,
            HttpLabAction::Failure,
        ] {
            let response = raw_reqwest_get(
                client.clone(),
                local_lab_url(target_action),
                cancellation.clone(),
                operation_id,
            )
            .await?;
            exchanges.push((target_action, response));
        }
        return Ok(exchanges);
    }

    let response = raw_reqwest_get(
        client,
        local_lab_url(action),
        cancellation.clone(),
        operation_id,
    )
    .await?;
    Ok(vec![(action, response)])
}

pub(crate) fn local_lab_url(action: HttpLabAction) -> String {
    match action {
        HttpLabAction::GetText => "https://httpbin.org/encoding/utf8".to_string(),
        HttpLabAction::GetJson => "https://httpbin.org/json".to_string(),
        HttpLabAction::GetXml => "https://httpbin.org/xml".to_string(),
        HttpLabAction::PostJson => "https://httpbin.org/post?local=post_json".to_string(),
        HttpLabAction::PostForm => "https://httpbin.org/post?local=post_form".to_string(),
        HttpLabAction::PostMultipart => "https://httpbin.org/post?local=multipart".to_string(),
        HttpLabAction::Cookies => "https://httpbin.org/cookies".to_string(),
        HttpLabAction::Failure => "https://httpbin.org/status/418".to_string(),
        HttpLabAction::FullFlow => TEST_URL.to_string(),
    }
}
