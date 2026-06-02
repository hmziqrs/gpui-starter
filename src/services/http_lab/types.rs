use serde::{Deserialize, Serialize};

use gpui_query::{CachePolicy, QueryKey, RequestPolicy, RetryPolicy};

const GET_CACHE_TTL_MS: u64 = 60_000;
const REVALIDATE_TTL_MS: u64 = 30_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpBodyKind {
    Text,
    Json,
    Xml,
}

impl HttpBodyKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::Xml => "xml",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpRequestBodyKind {
    None,
    Json,
    FormUrlEncoded,
    MultipartFormData,
}

impl HttpRequestBodyKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Json => "json",
            Self::FormUrlEncoded => "form-urlencoded",
            Self::MultipartFormData => "multipart/form-data",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequestSnapshot {
    pub method: String,
    pub url: String,
    pub request_body_kind: HttpRequestBodyKind,
    pub request_body_preview: String,
}

impl Default for HttpRequestSnapshot {
    fn default() -> Self {
        Self {
            method: String::new(),
            url: String::new(),
            request_body_kind: HttpRequestBodyKind::None,
            request_body_preview: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponseSnapshot {
    pub status: u16,
    pub status_text: String,
    pub final_url: String,
    pub elapsed_ms: u128,
    pub headers: Vec<(String, String)>,
    pub body_kind: HttpBodyKind,
    pub body_preview: String,
    pub parsed_json: Option<String>,
    pub parsed_xml_preview: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpExchange {
    pub label: String,
    pub request: HttpRequestSnapshot,
    pub response: Option<HttpResponseSnapshot>,
    pub error: Option<String>,
}

impl Default for HttpExchange {
    fn default() -> Self {
        Self {
            label: String::new(),
            request: HttpRequestSnapshot::default(),
            response: None,
            error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpCookieSnapshot {
    pub set_cookie_header: Option<String>,
    pub echoed_cookies_json: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HttpLabAction {
    GetText,
    GetJson,
    GetXml,
    PostJson,
    PostForm,
    PostMultipart,
    Cookies,
    Failure,
    FullFlow,
}

impl HttpLabAction {
    pub fn all() -> &'static [Self] {
        &[
            Self::GetText,
            Self::GetJson,
            Self::GetXml,
            Self::PostJson,
            Self::PostForm,
            Self::PostMultipart,
            Self::Cookies,
            Self::Failure,
            Self::FullFlow,
        ]
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::GetText => "get_text",
            Self::GetJson => "get_json",
            Self::GetXml => "get_xml",
            Self::PostJson => "post_json",
            Self::PostForm => "post_form",
            Self::PostMultipart => "post_multipart",
            Self::Cookies => "cookies",
            Self::Failure => "failure",
            Self::FullFlow => "full_flow",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::GetText => "GET text",
            Self::GetJson => "GET JSON",
            Self::GetXml => "GET XML",
            Self::PostJson => "POST JSON",
            Self::PostForm => "POST form",
            Self::PostMultipart => "POST multipart",
            Self::Cookies => "Cookies",
            Self::Failure => "Failure",
            Self::FullFlow => "Run full flow",
        }
    }

    pub fn method_label(self) -> &'static str {
        match self {
            Self::GetText | Self::GetJson | Self::GetXml | Self::Cookies | Self::Failure => "GET",
            Self::PostJson | Self::PostForm | Self::PostMultipart => "POST",
            Self::FullFlow => "FLOW",
        }
    }

    pub fn query_key(self) -> QueryKey {
        QueryKey::from(["http_lab", self.id()])
    }

    pub(super) fn cache_policy(self) -> CachePolicy {
        match self {
            Self::GetText | Self::GetXml => CachePolicy::Ttl {
                ttl_ms: GET_CACHE_TTL_MS,
            },
            Self::GetJson => CachePolicy::StaleWhileRevalidate {
                ttl_ms: REVALIDATE_TTL_MS,
            },
            Self::PostJson
            | Self::PostForm
            | Self::PostMultipart
            | Self::Cookies
            | Self::Failure
            | Self::FullFlow => CachePolicy::NoCache,
        }
    }

    pub(super) fn request_policy(self) -> RequestPolicy {
        match self {
            Self::PostMultipart | Self::FullFlow => RequestPolicy::IgnoreWhileLoading,
            _ => RequestPolicy::LatestWins,
        }
    }

    pub(super) fn retry_policy(self) -> RetryPolicy {
        match self {
            Self::GetText | Self::GetJson | Self::GetXml => RetryPolicy::new(3).with_delay(1000).with_exponential_backoff().with_max_delay(10_000),
            Self::Failure => RetryPolicy::new(2).with_delay(500).with_exponential_backoff().with_max_delay(5_000),
            _ => RetryPolicy::no_retries(), // POST actions, cookies, full flow: no retry
        }
    }
}

pub(super) type ActionExchange = (HttpLabAction, HttpExchange);
