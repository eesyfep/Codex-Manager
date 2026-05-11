use bytes::Bytes;
use codexmanager_core::storage::{now_ts, AggregateApi, Storage};
use reqwest::header::{HeaderName, HeaderValue};
use serde::Deserialize;
use std::collections::HashSet;
use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tiny_http::Request;

use super::super::GatewayUpstreamResponse;
use crate::aggregate_api::{
    AGGREGATE_API_AUTH_APIKEY, AGGREGATE_API_AUTH_USERPASS, AGGREGATE_API_PROVIDER_AZURE_OPENAI,
    AGGREGATE_API_PROVIDER_CLAUDE, AGGREGATE_API_PROVIDER_CODEX, AGGREGATE_API_PROVIDER_GEMINI,
};
use crate::gateway::protocol_adapter::{AdapterContract, ProviderFamily};
use crate::gateway::request_log::RequestLogUsage;

const AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL: usize = 3;
const AGGREGATE_API_POOL_WOOL: &str = "wool";
const BUFFERED_SSE_FAILOVER_MAX_BUFFER_MS: u64 = 25_000;
const LOW_QUALITY_RELAY_NO_UPSTREAM_AFTER_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);
const LOW_QUALITY_RELAY_RESPONSE_HEADER_TIMEOUT: Duration = Duration::from_secs(60);
const LOW_QUALITY_RELAY_RECENT_FAILURE_WINDOW_SECS: i64 = 5 * 60;
static WOOL_PREFLIGHT_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyAuthParams {
    location: String,
    name: String,
    #[serde(default)]
    header_value_format: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserPassAuthParams {
    mode: String,
    #[serde(default)]
    username_name: Option<String>,
    #[serde(default)]
    password_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserPassSecret {
    username: String,
    password: String,
}

#[derive(Debug, Clone)]
enum AggregateApiAuthConfig {
    ApiKeyDefaultBearer,
    ApiKeyHeader {
        name: String,
        format: String,
    },
    ApiKeyQuery {
        name: String,
    },
    UserPassBasic,
    UserPassHeaderPair {
        username_name: String,
        password_name: String,
    },
    UserPassQueryPair {
        username_name: String,
        password_name: String,
    },
}

fn normalize_header_key(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn normalize_action_path(action: &str) -> String {
    let action_trimmed = action.trim();
    if action_trimmed.is_empty() {
        return String::new();
    }
    if action_trimmed.starts_with('/') {
        action_trimmed.to_string()
    } else {
        format!("/{action_trimmed}")
    }
}

fn merge_action_query(original_path: &str, action: &str) -> String {
    let mut normalized = normalize_action_path(action);
    if normalized.is_empty() || normalized.contains('?') {
        return normalized;
    }
    if let Some((_, query)) = original_path.split_once('?') {
        if !query.trim().is_empty() {
            normalized.push('?');
            normalized.push_str(query);
        }
    }
    normalized
}

fn effective_action_path(candidate: &AggregateApi, path: &str) -> String {
    match candidate.action.as_deref().map(str::trim) {
        Some("") => path.to_string(),
        Some(value) => merge_action_query(path, value),
        None => path.to_string(),
    }
}

fn candidate_requires_responses_to_chat_adapter(
    storage: &Storage,
    model: Option<&str>,
    candidate: &AggregateApi,
    path: &str,
) -> bool {
    path.starts_with("/v1/responses")
        && crate::model_router::aggregate_candidate_requires_responses_to_chat_adapter(
            storage,
            model,
            candidate.id.as_str(),
        )
}

fn normalize_relay_log_url(value: &str) -> String {
    let trimmed = value.trim();
    if let Ok(mut url) = reqwest::Url::parse(trimmed) {
        url.set_query(None);
        url.set_fragment(None);
        let path = url.path().trim_end_matches('/').to_string();
        let base_path = path
            .strip_suffix("/v1/responses")
            .or_else(|| path.strip_suffix("/v1/chat/completions"))
            .or_else(|| path.strip_suffix("/responses"))
            .or_else(|| path.strip_suffix("/chat/completions"))
            .or_else(|| path.strip_suffix("/v1"))
            .unwrap_or(&path);
        url.set_path(if base_path.is_empty() { "/" } else { base_path });
        return url.as_str().trim_end_matches('/').to_ascii_lowercase();
    }
    trimmed
        .trim()
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn low_quality_relay_error_indicates_stream_failure(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("response.failed")
        || normalized.contains("bridge_stage=after_downstream_response_started")
        || normalized.contains("stream_terminal_seen=false")
        || normalized.contains("no_upstream_after_handshake")
        || normalized.contains("上游首帧等待超时")
        || normalized.contains("connection reset")
        || normalized.contains("disconnect")
        || normalized.contains("timed out")
        || normalized.contains("timeout")
}

fn candidate_has_recent_low_quality_passthrough_failure(
    storage: &Storage,
    model: Option<&str>,
    candidate: &AggregateApi,
    request_path: &str,
) -> bool {
    if !candidate.compatibility_mode || !request_path.starts_with("/v1/responses") {
        return false;
    }
    let logs = match storage.list_request_logs(Some("adapter:=Passthrough"), 100) {
        Ok(logs) => logs,
        Err(err) => {
            log::warn!(
                "event=low_quality_relay_recent_failure_lookup_failed aggregate_api_id={} error={}",
                candidate.id,
                err
            );
            return false;
        }
    };
    let now = now_ts();
    let candidate_url = normalize_relay_log_url(candidate.url.as_str());
    let requested_model = model.map(str::trim).filter(|value| !value.is_empty());

    for log in logs {
        if now.saturating_sub(log.created_at) > LOW_QUALITY_RELAY_RECENT_FAILURE_WINDOW_SECS {
            return false;
        }
        if !log
            .original_path
            .as_deref()
            .or(Some(log.request_path.as_str()))
            .is_some_and(|path| path.starts_with("/v1/responses"))
        {
            continue;
        }
        if requested_model
            .zip(log.model.as_deref())
            .is_some_and(|(expected, actual)| expected != actual.trim())
        {
            continue;
        }
        let same_candidate = log
            .aggregate_api_url
            .as_deref()
            .or(log.upstream_url.as_deref())
            .map(normalize_relay_log_url)
            .is_some_and(|url| url == candidate_url);
        if !same_candidate {
            continue;
        }
        return matches!(log.status_code, Some(status_code) if status_code >= 500)
            && log
                .error
                .as_deref()
                .is_some_and(low_quality_relay_error_indicates_stream_failure);
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AggregateProviderContract {
    provider_family: ProviderFamily,
    default_contract: AdapterContract,
    allows_azure_openai_compat: bool,
}

fn resolve_provider_family(value: &str) -> ProviderFamily {
    match normalize_provider_type_value(value).as_str() {
        AGGREGATE_API_PROVIDER_CLAUDE => ProviderFamily::Anthropic,
        AGGREGATE_API_PROVIDER_GEMINI => ProviderFamily::Gemini,
        _ => ProviderFamily::OpenAI,
    }
}

fn aggregate_provider_contract_for_protocol(protocol_type: &str) -> AggregateProviderContract {
    match protocol_type {
        "anthropic_native" => AggregateProviderContract {
            provider_family: ProviderFamily::Anthropic,
            default_contract: AdapterContract::anthropic_native(),
            allows_azure_openai_compat: false,
        },
        "gemini_native" => AggregateProviderContract {
            provider_family: ProviderFamily::Gemini,
            default_contract: AdapterContract::gemini_native(),
            allows_azure_openai_compat: false,
        },
        _ => AggregateProviderContract {
            provider_family: ProviderFamily::OpenAI,
            default_contract: AdapterContract::native_openai_responses_passthrough(),
            allows_azure_openai_compat: true,
        },
    }
}

fn aggregate_candidate_matches_provider_contract(
    candidate: &AggregateApi,
    contract: AggregateProviderContract,
) -> bool {
    let normalized_provider = normalize_provider_type_value(candidate.provider_type.as_str());
    let candidate_family = resolve_provider_family(candidate.provider_type.as_str());
    candidate.status == "active"
        && (candidate_family == contract.provider_family
            || (contract.provider_family == ProviderFamily::OpenAI
                && contract.allows_azure_openai_compat
                && normalized_provider == AGGREGATE_API_PROVIDER_AZURE_OPENAI))
}

fn aggregate_candidate_adapter_contract(
    storage: &Storage,
    model: Option<&str>,
    candidate: &AggregateApi,
    request_path: &str,
    default_contract: AdapterContract,
) -> AdapterContract {
    if candidate_requires_responses_to_chat_adapter(storage, model, candidate, request_path) {
        return AdapterContract::responses_from_chat(resolve_provider_family(
            candidate.provider_type.as_str(),
        ));
    }
    if candidate_has_recent_low_quality_passthrough_failure(storage, model, candidate, request_path)
    {
        return AdapterContract::responses_from_streaming_chat(resolve_provider_family(
            candidate.provider_type.as_str(),
        ));
    }
    default_contract
}

fn responses_to_chat_candidate_body(body: &Bytes, upstream_stream: bool) -> Result<Bytes, String> {
    super::super::super::request_rewrite::rewrite_responses_body_to_chat_completions(
        body,
        upstream_stream,
    )
}

fn aggregate_passthrough_candidate_body(body: &Bytes, path: &str) -> Bytes {
    if path.starts_with("/v1/responses")
        && super::super::support::payload_rewrite::body_has_encrypted_content_hint(body.as_ref())
    {
        return super::super::support::payload_rewrite::strip_encrypted_content_from_body(
            body.as_ref(),
        )
        .map(Bytes::from)
        .unwrap_or_else(|| body.clone());
    }
    body.clone()
}

fn apply_candidate_fast_service_tier(candidate: &AggregateApi, body: Bytes) -> Bytes {
    if !candidate.fast {
        return body;
    }
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body.as_ref()) else {
        return body;
    };
    let Some(object) = value.as_object_mut() else {
        return body;
    };
    if object.contains_key("service_tier") {
        return body;
    }
    object.insert(
        "service_tier".to_string(),
        serde_json::Value::String("fast".to_string()),
    );
    serde_json::to_vec(&value).map(Bytes::from).unwrap_or(body)
}

fn apply_candidate_upstream_model(body: Bytes, upstream_model: Option<&str>) -> Bytes {
    let Some(upstream_model) = upstream_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return body;
    };
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body.as_ref()) else {
        return body;
    };
    let Some(object) = value.as_object_mut() else {
        return body;
    };
    object.insert(
        "model".to_string(),
        serde_json::Value::String(upstream_model.to_string()),
    );
    serde_json::to_vec(&value).map(Bytes::from).unwrap_or(body)
}

fn candidate_needs_buffered_sse_failover(
    _candidate: &AggregateApi,
    _is_stream: bool,
    _has_more_candidates: bool,
) -> bool {
    // User-facing compatibility mode now means low-quality relay streaming
    // protection. It must not opt a candidate into the old buffered terminal
    // validation guard, which can leave Codex waiting behind a relay buffer.
    false
}

fn buffered_sse_failover_deadline(request_deadline: Option<Instant>) -> Instant {
    let buffer_deadline =
        Instant::now() + Duration::from_millis(BUFFERED_SSE_FAILOVER_MAX_BUFFER_MS);
    request_deadline
        .filter(|deadline| *deadline < buffer_deadline)
        .unwrap_or(buffer_deadline)
}

fn low_quality_relay_response_header_deadline(request_deadline: Option<Instant>) -> Instant {
    let relay_deadline = Instant::now() + LOW_QUALITY_RELAY_RESPONSE_HEADER_TIMEOUT;
    request_deadline
        .filter(|deadline| *deadline < relay_deadline)
        .unwrap_or(relay_deadline)
}

fn effective_response_header_deadline(
    candidate: &AggregateApi,
    upstream_path: &str,
    upstream_is_stream: bool,
    request_deadline: Option<Instant>,
) -> Option<Instant> {
    if candidate.compatibility_mode
        && upstream_is_stream
        && upstream_path.starts_with("/v1/responses")
    {
        Some(low_quality_relay_response_header_deadline(request_deadline))
    } else {
        request_deadline
    }
}

#[derive(Debug)]
struct BufferedSseCollectResult {
    body: Bytes,
    terminal_seen: bool,
    last_sse_event: Option<String>,
    event_tail: Vec<String>,
    elapsed_ms: u128,
    read_error: Option<String>,
}

fn collect_buffered_sse_upstream(
    mut upstream: reqwest::blocking::Response,
) -> BufferedSseCollectResult {
    let started_at = Instant::now();
    let mut body = Vec::new();
    let read_error = upstream
        .read_to_end(&mut body)
        .err()
        .map(|err| format!("read buffered upstream stream failed: {err}"));
    let text = String::from_utf8_lossy(body.as_slice());
    let last_sse_event = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("event:").map(str::trim))
        .filter(|value| !value.is_empty())
        .last()
        .map(str::to_string);
    let mut event_tail = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("event:").map(str::trim))
        .filter(|value| !value.is_empty())
        .rev()
        .take(6)
        .map(str::to_string)
        .collect::<Vec<_>>();
    event_tail.reverse();
    let terminal_seen = last_sse_event
        .as_deref()
        .is_some_and(|event| matches!(event, "response.completed" | "response.done"));
    BufferedSseCollectResult {
        body: Bytes::from(body),
        terminal_seen,
        last_sse_event,
        event_tail,
        elapsed_ms: started_at.elapsed().as_millis(),
        read_error,
    }
}

fn build_upstream_url(base_url: &str, effective_path: &str) -> Result<reqwest::Url, ()> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| ())?;
    let trimmed_path = effective_path.trim();
    if trimmed_path.is_empty() {
        return Ok(url);
    }
    let (path_part, query_part) = trimmed_path
        .split_once('?')
        .map_or((trimmed_path, None), |(path, query)| (path, Some(query)));
    let suffix = path_part.trim_start_matches('/');
    let base_path = url.path().trim_end_matches('/').to_string();
    let combined_path = if base_path.is_empty() || base_path == "/" {
        format!("/{}", suffix)
    } else if base_path.ends_with("/v1") && suffix == "v1" {
        base_path
    } else if base_path.ends_with("/v1") && suffix.starts_with("v1/") {
        format!("{}/{}", base_path, suffix.trim_start_matches("v1/"))
    } else if suffix.is_empty() {
        base_path
    } else {
        format!("{}/{}", base_path, suffix)
    };
    url.set_path(combined_path.as_str());
    url.set_query(query_part.filter(|query| !query.trim().is_empty()));
    Ok(url)
}

fn replace_query_param(mut url: reqwest::Url, name: &str, value: &str) -> reqwest::Url {
    let name_trimmed = name.trim();
    if name_trimmed.is_empty() {
        return url;
    }
    let existing = url.query_pairs().into_owned().collect::<Vec<_>>();
    url.set_query(None);
    {
        let mut qp = url.query_pairs_mut();
        for (k, v) in existing {
            if k == name_trimmed {
                continue;
            }
            qp.append_pair(k.as_str(), v.as_str());
        }
        qp.append_pair(name_trimmed, value);
    }
    url
}

fn redacted_url_for_log(url: &reqwest::Url) -> String {
    let mut redacted = url.clone();
    let query_pairs = redacted.query_pairs().into_owned().collect::<Vec<_>>();
    if query_pairs.is_empty() {
        return redacted.as_str().to_string();
    }
    redacted.set_query(None);
    {
        let mut query = redacted.query_pairs_mut();
        for (name, value) in query_pairs {
            let normalized = name.trim().to_ascii_lowercase();
            let is_sensitive = matches!(
                normalized.as_str(),
                "key"
                    | "api_key"
                    | "apikey"
                    | "api-key"
                    | "x-api-key"
                    | "access_token"
                    | "token"
                    | "secret"
                    | "password"
                    | "pass"
            );
            query.append_pair(
                name.as_str(),
                if is_sensitive {
                    "<redacted>"
                } else {
                    value.as_str()
                },
            );
        }
    }
    redacted.as_str().to_string()
}

fn parse_auth_config(
    candidate: &AggregateApi,
) -> Result<(AggregateApiAuthConfig, HashSet<String>), String> {
    let auth_type = candidate.auth_type.trim().to_ascii_lowercase();
    let raw_params = candidate
        .auth_params_json
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut injected_headers = HashSet::new();

    if raw_params.is_none() {
        if auth_type == AGGREGATE_API_AUTH_USERPASS {
            return Ok((AggregateApiAuthConfig::UserPassBasic, injected_headers));
        }
        if normalize_provider_type_value(candidate.provider_type.as_str())
            == AGGREGATE_API_PROVIDER_AZURE_OPENAI
        {
            injected_headers.insert("api-key".to_string());
            return Ok((
                AggregateApiAuthConfig::ApiKeyHeader {
                    name: "api-key".to_string(),
                    format: "raw".to_string(),
                },
                injected_headers,
            ));
        }
        return Ok((
            AggregateApiAuthConfig::ApiKeyDefaultBearer,
            injected_headers,
        ));
    }

    let value: serde_json::Value = serde_json::from_str(raw_params.unwrap())
        .map_err(|_| "invalid aggregate api authParams".to_string())?;

    if auth_type == AGGREGATE_API_AUTH_APIKEY {
        let parsed: ApiKeyAuthParams = serde_json::from_value(value)
            .map_err(|_| "invalid aggregate api authParams".to_string())?;
        let location = parsed.location.trim().to_ascii_lowercase();
        if location == "query" {
            return Ok((
                AggregateApiAuthConfig::ApiKeyQuery {
                    name: parsed.name.trim().to_string(),
                },
                injected_headers,
            ));
        }
        let header_name = parsed.name.trim().to_string();
        injected_headers.insert(normalize_header_key(header_name.as_str()));
        let format = parsed
            .header_value_format
            .as_deref()
            .unwrap_or("bearer")
            .trim()
            .to_ascii_lowercase();
        return Ok((
            AggregateApiAuthConfig::ApiKeyHeader {
                name: header_name,
                format,
            },
            injected_headers,
        ));
    }

    if auth_type == AGGREGATE_API_AUTH_USERPASS {
        let parsed: UserPassAuthParams = serde_json::from_value(value)
            .map_err(|_| "invalid aggregate api authParams".to_string())?;
        let mode = parsed.mode.trim().to_ascii_lowercase();
        match mode.as_str() {
            "basic" => return Ok((AggregateApiAuthConfig::UserPassBasic, injected_headers)),
            "headerpair" => {
                let username_name = parsed
                    .username_name
                    .as_deref()
                    .unwrap_or("username")
                    .trim()
                    .to_string();
                let password_name = parsed
                    .password_name
                    .as_deref()
                    .unwrap_or("password")
                    .trim()
                    .to_string();
                injected_headers.insert(normalize_header_key(username_name.as_str()));
                injected_headers.insert(normalize_header_key(password_name.as_str()));
                return Ok((
                    AggregateApiAuthConfig::UserPassHeaderPair {
                        username_name,
                        password_name,
                    },
                    injected_headers,
                ));
            }
            "querypair" => {
                let username_name = parsed
                    .username_name
                    .as_deref()
                    .unwrap_or("username")
                    .trim()
                    .to_string();
                let password_name = parsed
                    .password_name
                    .as_deref()
                    .unwrap_or("password")
                    .trim()
                    .to_string();
                return Ok((
                    AggregateApiAuthConfig::UserPassQueryPair {
                        username_name,
                        password_name,
                    },
                    injected_headers,
                ));
            }
            _ => return Err("invalid aggregate api authParams".to_string()),
        }
    }

    Ok((
        AggregateApiAuthConfig::ApiKeyDefaultBearer,
        injected_headers,
    ))
}

fn resolve_passthrough_sse_protocol(
    candidate: &AggregateApi,
    path: &str,
    response_adapter: super::super::super::ResponseAdapter,
) -> Option<super::super::super::PassthroughSseProtocol> {
    if response_adapter != super::super::super::ResponseAdapter::Passthrough {
        return None;
    }
    let provider_type = normalize_provider_type_value(candidate.provider_type.as_str());
    if provider_type != AGGREGATE_API_PROVIDER_CLAUDE {
        return None;
    }
    if path == "/v1/messages" || path.starts_with("/v1/messages?") {
        return Some(super::super::super::PassthroughSseProtocol::AnthropicNative);
    }
    None
}

/// 函数 `should_skip_forward_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn should_skip_forward_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "x-api-key"
            | "api-key"
            | "content-length"
            | "connection"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
    )
}

fn should_skip_forward_header_with_overrides(name: &str, injected: &HashSet<String>) -> bool {
    if should_skip_forward_header(name) {
        return true;
    }
    injected.contains(normalize_header_key(name).as_str())
}

fn should_skip_forward_header_for_aggregate_request(
    name: &str,
    injected: &HashSet<String>,
    is_stream: bool,
) -> bool {
    if should_skip_forward_header_with_overrides(name, injected) {
        return true;
    }
    let normalized = normalize_header_key(name);
    normalized == "accept-encoding" || (is_stream && normalized == "accept")
}

/// 函数 `respond_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - request: 参数 request
/// - status: 参数 status
/// - message: 参数 message
/// - trace_id: 参数 trace_id
///
/// # 返回
/// 无
fn respond_error(request: Request, status: u16, message: &str, trace_id: Option<&str>) {
    let response_message = super::super::super::error_message_for_client(
        super::super::super::prefers_raw_errors_for_tiny_http_request(&request),
        message,
    );
    let response = super::super::super::error_response::terminal_text_response(
        status,
        response_message,
        trace_id,
    );
    let _ = request.respond(response);
}

/// 函数 `normalize_candidate_order`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - candidates: 参数 candidates
///
/// # 返回
/// 返回函数执行结果
fn normalize_candidate_order(mut candidates: Vec<AggregateApi>) -> Vec<AggregateApi> {
    candidates.sort_by(|left, right| {
        left.sort
            .cmp(&right.sort)
            .then(right.created_at.cmp(&left.created_at))
            .then(left.id.cmp(&right.id))
    });
    candidates
}

fn is_wool_candidate(candidate: &AggregateApi) -> bool {
    candidate
        .pool
        .trim()
        .eq_ignore_ascii_case(AGGREGATE_API_POOL_WOOL)
}

fn now_unix_ts() -> i64 {
    codexmanager_core::storage::now_ts()
}

fn wool_candidate_in_cooldown(candidate: &AggregateApi) -> bool {
    candidate
        .wool_cooldown_until
        .is_some_and(|until| until > now_unix_ts())
}

fn wool_preflight_fresh(candidate: &AggregateApi) -> bool {
    let Some(last_at) = candidate.wool_last_preflight_at else {
        return false;
    };
    let ttl = super::super::super::wool_preflight_ttl_seconds() as i64;
    now_unix_ts().saturating_sub(last_at) <= ttl
}

fn wool_cooldown_until() -> i64 {
    now_unix_ts().saturating_add(super::super::super::wool_cooldown_seconds() as i64)
}

fn mark_wool_failure(storage: &Storage, candidate: &AggregateApi) {
    let next_failure_count = candidate.wool_failure_count.saturating_add(1);
    let threshold = super::super::super::wool_failure_threshold() as i64;
    let cooldown_until = if next_failure_count >= threshold {
        Some(wool_cooldown_until())
    } else {
        None
    };
    let _ = storage.mark_aggregate_api_wool_failure(candidate.id.as_str(), cooldown_until);
}

fn ensure_wool_preflight(
    storage: &Storage,
    candidate: &AggregateApi,
    secret: &str,
    client: &reqwest::blocking::Client,
) -> bool {
    if !is_wool_candidate(candidate) {
        return true;
    }
    if wool_preflight_fresh(candidate) {
        return true;
    }
    let Some(_guard) =
        super::super::super::acquire_wool_preflight(super::super::super::wool_preflight_workers())
    else {
        return false;
    };
    let _lock = match WOOL_PREFLIGHT_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Ok(Some(refreshed)) = storage.find_aggregate_api_by_id(candidate.id.as_str()) {
        if wool_preflight_fresh(&refreshed) {
            return true;
        }
        if wool_candidate_in_cooldown(&refreshed) {
            return false;
        }
    }
    let result = match normalize_provider_type_value(candidate.provider_type.as_str()).as_str() {
        AGGREGATE_API_PROVIDER_CLAUDE => {
            crate::aggregate_api::probe_claude_endpoint(client, candidate, secret)
        }
        AGGREGATE_API_PROVIDER_GEMINI => {
            crate::aggregate_api::probe_gemini_endpoint(client, candidate, secret)
        }
        _ => crate::aggregate_api::probe_codex_endpoint(client, candidate, secret),
    };
    match result {
        Ok(_) => {
            let _ = storage.mark_aggregate_api_wool_preflight_success(candidate.id.as_str());
            true
        }
        Err(_) => {
            let cooldown_until = Some(wool_cooldown_until());
            let _ = storage.mark_aggregate_api_wool_failure(candidate.id.as_str(), cooldown_until);
            false
        }
    }
}

fn wool_per_api_limit(candidate: &AggregateApi) -> usize {
    candidate
        .wool_max_inflight
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(super::super::super::wool_max_inflight_per_api)
}

fn order_wool_then_primary(candidates: Vec<AggregateApi>) -> Vec<AggregateApi> {
    if !super::super::super::wool_enabled() {
        return candidates
            .into_iter()
            .filter(|candidate| !is_wool_candidate(candidate))
            .collect();
    }
    let mut wool = Vec::new();
    let mut primary = Vec::new();
    for candidate in candidates {
        if is_wool_candidate(&candidate) {
            wool.push(candidate);
        } else {
            primary.push(candidate);
        }
    }
    wool.extend(primary);
    wool
}

/// 函数 `apply_gateway_route_strategy_to_aggregate_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn apply_gateway_route_strategy_to_aggregate_candidates(
    candidates: &mut [AggregateApi],
    key_id: &str,
    model: Option<&str>,
    preferred_aggregate_api_id: Option<&str>,
) {
    if candidates.len() <= 1 {
        return;
    }
    if crate::gateway::current_route_strategy() != "balanced" {
        return;
    }

    let preferred_id = preferred_aggregate_api_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let preserves_head = preferred_id
        .and_then(|preferred_id| candidates.first().map(|first| (preferred_id, first)))
        .is_some_and(|(preferred_id, first)| first.id == preferred_id);
    let scope = candidates
        .iter()
        .map(|candidate| {
            format!(
                "{}@{}",
                candidate.id.trim(),
                candidate.url.trim().trim_end_matches('/')
            )
        })
        .collect::<Vec<_>>()
        .join("|");

    if preserves_head {
        if candidates.len() > 1 {
            super::super::super::route_hint::apply_balanced_round_robin_with_scope(
                &mut candidates[1..],
                key_id,
                model,
                scope.as_str(),
            );
        }
    } else {
        super::super::super::route_hint::apply_balanced_round_robin_with_scope(
            candidates,
            key_id,
            model,
            scope.as_str(),
        );
    }
}

/// 函数 `normalize_provider_type_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_provider_type_value(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    match normalized.as_str() {
        "claude" | "anthropic" | "anthropic_native" | "claude_code" => {
            AGGREGATE_API_PROVIDER_CLAUDE.to_string()
        }
        "gemini" | "gemini_native" | "google" | "google_ai" | "google_gemini" => {
            AGGREGATE_API_PROVIDER_GEMINI.to_string()
        }
        "azure" | "azure_openai" | "azure_openai_compat" => {
            AGGREGATE_API_PROVIDER_AZURE_OPENAI.to_string()
        }
        _ => AGGREGATE_API_PROVIDER_CODEX.to_string(),
    }
}

/// 函数 `first_upstream_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
/// - names: 参数 names
///
/// # 返回
/// 返回函数执行结果
fn first_upstream_header(headers: &reqwest::header::HeaderMap, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

/// 函数 `aggregate_api_failure_message`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status_code: 参数 status_code
/// - body: 参数 body
/// - request_id: 参数 request_id
/// - cf_ray: 参数 cf_ray
/// - auth_error: 参数 auth_error
/// - identity_error_code: 参数 identity_error_code
///
/// # 返回
/// 返回函数执行结果
fn aggregate_api_failure_message(
    status_code: u16,
    body: &[u8],
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> String {
    let mut parts =
        vec![
            crate::gateway::summarize_upstream_error_hint_from_body(status_code, body)
                .unwrap_or_else(|| format!("aggregate api upstream status={status_code}")),
        ];
    if let Some(request_id) = request_id.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("request_id={request_id}"));
    }
    if let Some(cf_ray) = cf_ray.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("cf_ray={cf_ray}"));
    }
    if let Some(auth_error) = auth_error.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("auth_error={auth_error}"));
    }
    if let Some(identity_error_code) = identity_error_code
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("identity_error_code={identity_error_code}"));
    }
    if parts.len() == 1 {
        parts.remove(0)
    } else {
        format!("{} [{}]", parts.remove(0), parts.join(", "))
    }
}

/// 函数 `build_aggregate_api_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - client: 参数 client
/// - request: 参数 request
/// - method: 参数 method
/// - url: 参数 url
/// - body: 参数 body
/// - secret: 参数 secret
/// - request_deadline: 参数 request_deadline
/// - is_stream: 参数 is_stream
///
/// # 返回
/// 返回函数执行结果
fn build_aggregate_api_request(
    client: &reqwest::blocking::Client,
    request: &Request,
    method: &reqwest::Method,
    url: reqwest::Url,
    body: &Bytes,
    secret: &str,
    auth_config: &AggregateApiAuthConfig,
    injected_headers: &HashSet<String>,
    request_deadline: Option<Instant>,
    is_stream: bool,
) -> Result<reqwest::blocking::RequestBuilder, String> {
    let mut builder = client.request(method.clone(), url);
    if let Some(timeout) =
        super::super::support::deadline::send_timeout(request_deadline, is_stream)
    {
        builder = builder.timeout(timeout);
    }
    let request_headers = request.headers().to_vec();
    for header in &request_headers {
        if should_skip_forward_header_for_aggregate_request(
            header.field.as_str().into(),
            injected_headers,
            is_stream,
        ) {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(header.field.as_str().as_bytes()),
            HeaderValue::from_str(header.value.as_str()),
        ) {
            builder = builder.header(name, value);
        }
    }
    if is_stream {
        builder = builder.header(
            HeaderName::from_static("accept"),
            HeaderValue::from_static("text/event-stream"),
        );
    }

    let secret_trimmed = secret.trim();
    match auth_config {
        AggregateApiAuthConfig::ApiKeyDefaultBearer => {
            builder = builder.header(
                HeaderName::from_static("authorization"),
                HeaderValue::from_str(format!("Bearer {}", secret_trimmed).as_str())
                    .map_err(|_| "invalid aggregate api secret".to_string())?,
            );
        }
        AggregateApiAuthConfig::ApiKeyHeader { name, format } => {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| "invalid aggregate api auth header".to_string())?;
            let value = if format == "raw" {
                secret_trimmed.to_string()
            } else {
                format!("Bearer {}", secret_trimmed)
            };
            builder = builder.header(
                header_name,
                HeaderValue::from_str(value.as_str())
                    .map_err(|_| "invalid aggregate api secret".to_string())?,
            );
        }
        AggregateApiAuthConfig::ApiKeyQuery { .. } => {}
        AggregateApiAuthConfig::UserPassBasic
        | AggregateApiAuthConfig::UserPassHeaderPair { .. }
        | AggregateApiAuthConfig::UserPassQueryPair { .. } => {
            let parsed: UserPassSecret = serde_json::from_str(secret_trimmed)
                .map_err(|_| "invalid aggregate api secret".to_string())?;
            match auth_config {
                AggregateApiAuthConfig::UserPassBasic => {
                    builder = builder.basic_auth(parsed.username, Some(parsed.password));
                }
                AggregateApiAuthConfig::UserPassHeaderPair {
                    username_name,
                    password_name,
                } => {
                    let user_header = HeaderName::from_bytes(username_name.as_bytes())
                        .map_err(|_| "invalid aggregate api auth header".to_string())?;
                    let pass_header = HeaderName::from_bytes(password_name.as_bytes())
                        .map_err(|_| "invalid aggregate api auth header".to_string())?;
                    builder = builder.header(
                        user_header,
                        HeaderValue::from_str(parsed.username.as_str())
                            .map_err(|_| "invalid aggregate api secret".to_string())?,
                    );
                    builder = builder.header(
                        pass_header,
                        HeaderValue::from_str(parsed.password.as_str())
                            .map_err(|_| "invalid aggregate api secret".to_string())?,
                    );
                }
                AggregateApiAuthConfig::UserPassQueryPair { .. } => {}
                _ => {}
            }
        }
    }
    if !body.is_empty() {
        builder = builder.body(body.clone());
    }
    Ok(builder)
}

/// 函数 `resolve_aggregate_api_rotation_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn resolve_aggregate_api_rotation_candidates(
    storage: &Storage,
    protocol_type: &str,
    aggregate_api_id: Option<&str>,
) -> Result<Vec<AggregateApi>, String> {
    let provider_contract = aggregate_provider_contract_for_protocol(protocol_type);

    let mut candidates = storage
        .list_aggregate_apis()
        .map_err(|err| err.to_string())?
        .into_iter()
        .filter(|api| aggregate_candidate_matches_provider_contract(api, provider_contract))
        .collect::<Vec<_>>();
    candidates = normalize_candidate_order(candidates);

    if let Some(api_id) = aggregate_api_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(preferred) = storage
            .find_aggregate_api_by_id(api_id)
            .map_err(|err| err.to_string())?
            .filter(|api| api.status.trim().eq_ignore_ascii_case("active"))
        {
            candidates.retain(|api| api.id != preferred.id);
            candidates.insert(0, preferred);
        }
    }

    if candidates.is_empty() {
        Err(format!(
            "aggregate api not found for provider {:?}",
            provider_contract.provider_family
        ))
    } else {
        Ok(candidates)
    }
}

/// 函数 `proxy_aggregate_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - in super: 参数 in super
///
/// # 返回
/// 返回函数执行结果
pub(in super::super) struct AggregateProxyRequest<'a> {
    pub request: Request,
    pub storage: &'a Storage,
    pub trace_id: &'a str,
    pub key_id: &'a str,
    pub original_path: &'a str,
    pub path: &'a str,
    pub request_method: &'a str,
    pub method: &'a reqwest::Method,
    pub body: &'a Bytes,
    pub is_stream: bool,
    pub response_adapter: super::super::super::ResponseAdapter,
    pub model_for_log: Option<&'a str>,
    pub reasoning_for_log: Option<&'a str>,
    pub effective_service_tier_for_log: Option<&'a str>,
    pub conversation_id: Option<&'a str>,
    pub aggregate_api_candidates: Vec<AggregateApi>,
    pub request_deadline: Option<Instant>,
    pub started_at: Instant,
}

pub(in super::super) fn proxy_aggregate_request(
    params: AggregateProxyRequest<'_>,
) -> Result<(), String> {
    let AggregateProxyRequest {
        request,
        storage,
        trace_id,
        key_id,
        original_path,
        path,
        request_method,
        method,
        body,
        is_stream,
        response_adapter,
        model_for_log,
        reasoning_for_log,
        effective_service_tier_for_log,
        conversation_id,
        aggregate_api_candidates,
        request_deadline,
        started_at,
    } = params;
    if aggregate_api_candidates.is_empty() {
        let message = "aggregate api not found".to_string();
        super::super::super::record_gateway_request_outcome(path, 404, Some("aggregate_api"));
        super::super::super::trace_log::log_request_final(
            trace_id,
            404,
            Some(key_id),
            None,
            Some(message.as_str()),
            started_at.elapsed().as_millis(),
        );
        let request = request;
        respond_error(request, 404, message.as_str(), Some(trace_id));
        return Ok(());
    }

    let client = super::super::super::fresh_upstream_client();
    let mut request = Some(request);
    let mut attempted_aggregate_api_ids = Vec::new();
    let mut last_attempt_url: Option<String> = None;
    let mut last_attempt_supplier_name: Option<String> = None;
    let mut last_attempt_error: Option<String> = None;
    let mut last_failure_status = 502u16;
    let mut wool_skipped_cooldown: usize = 0;
    let mut wool_skipped_inflight: usize = 0;
    let mut wool_skipped_preflight: usize = 0;
    let mut primary_attempted: usize = 0;
    let mut wool_candidates_for_rescan: Vec<AggregateApi> = Vec::new();

    let aggregate_api_candidates = order_wool_then_primary(aggregate_api_candidates);
    let total_candidates = aggregate_api_candidates.len();
    for (candidate_idx, candidate) in aggregate_api_candidates.into_iter().enumerate() {
        let has_more_candidates = candidate_idx + 1 < total_candidates;
        let is_wool = is_wool_candidate(&candidate);
        if is_wool {
            wool_candidates_for_rescan.push(candidate.clone());
        }
        if is_wool && wool_candidate_in_cooldown(&candidate) {
            wool_skipped_cooldown += 1;
            super::super::super::record_gateway_candidate_skip(
                super::super::super::GatewayCandidateSkipReason::Cooldown,
            );
            continue;
        }
        let _wool_inflight_guard = if is_wool {
            let Some(guard) = super::super::super::acquire_wool_inflight(
                candidate.id.as_str(),
                wool_per_api_limit(&candidate),
                super::super::super::wool_pool_max_inflight(),
            ) else {
                wool_skipped_inflight += 1;
                super::super::super::record_gateway_candidate_skip(
                    super::super::super::GatewayCandidateSkipReason::Inflight,
                );
                continue;
            };
            Some(guard)
        } else {
            None
        };
        if !is_wool {
            primary_attempted += 1;
        }
        attempted_aggregate_api_ids.push(candidate.id.clone());
        let candidate_supplier_name = candidate.supplier_name.clone();
        let candidate_url = candidate.url.clone();
        let Some(secret) = storage
            .find_aggregate_api_secret_by_id(candidate.id.as_str())
            .map_err(|err| err.to_string())?
        else {
            crate::model_router::record_route_binding_error(
                storage,
                model_for_log,
                candidate.id.as_str(),
                "aggregate api secret not found",
            );
            last_attempt_url = Some(candidate_url.clone());
            last_attempt_supplier_name = candidate_supplier_name.clone();
            last_attempt_error = Some("aggregate api secret not found".to_string());
            last_failure_status = 403;
            continue;
        };

        if is_wool && !ensure_wool_preflight(storage, &candidate, secret.as_str(), &client) {
            wool_skipped_preflight += 1;
            super::super::super::record_gateway_candidate_skip(
                super::super::super::GatewayCandidateSkipReason::Cooldown,
            );
            last_attempt_url = Some(candidate_url.clone());
            last_attempt_supplier_name = candidate_supplier_name.clone();
            last_attempt_error = Some("wool aggregate api preflight failed".to_string());
            last_failure_status = 502;
            continue;
        }

        let candidate_contract = aggregate_candidate_adapter_contract(
            storage,
            model_for_log,
            &candidate,
            path,
            AdapterContract::native_openai_responses_passthrough(),
        );
        let upstream_path = candidate_contract.upstream_path_for(path);
        let effective_path = effective_action_path(&candidate, upstream_path);
        let candidate_response_adapter = candidate_contract.response_adapter();
        let _ = candidate_contract.provider_family;
        let upstream_is_stream =
            is_stream && !candidate_contract.disables_upstream_stream_passthrough();
        let candidate_body = if candidate_contract.requires_responses_to_chat_rewrite() {
            match responses_to_chat_candidate_body(body, upstream_is_stream) {
                Ok(value) => value,
                Err(err) => {
                    crate::model_router::record_route_binding_error(
                        storage,
                        model_for_log,
                        candidate.id.as_str(),
                        err.as_str(),
                    );
                    last_attempt_url = Some(candidate_url.clone());
                    last_attempt_supplier_name = candidate_supplier_name.clone();
                    last_attempt_error = Some(format!("responses_to_chat_adapter_failed: {err}"));
                    last_failure_status = 400;
                    continue;
                }
            }
        } else {
            aggregate_passthrough_candidate_body(body, path)
        };
        let upstream_model = crate::model_router::resolve_upstream_model_for_aggregate_candidate(
            storage,
            model_for_log,
            candidate.id.as_str(),
        );
        let candidate_body =
            apply_candidate_upstream_model(candidate_body, upstream_model.as_deref());
        let candidate_body = apply_candidate_fast_service_tier(&candidate, candidate_body);
        let (auth_config, injected_headers) = match parse_auth_config(&candidate) {
            Ok(value) => value,
            Err(err) => {
                crate::model_router::record_route_binding_error(
                    storage,
                    model_for_log,
                    candidate.id.as_str(),
                    err.as_str(),
                );
                last_attempt_url = Some(candidate_url.clone());
                last_attempt_supplier_name = candidate_supplier_name.clone();
                last_attempt_error = Some(err);
                last_failure_status = 502;
                continue;
            }
        };

        let mut succeeded = false;
        for attempt_idx in 0..=AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
            if super::super::support::deadline::is_expired(request_deadline) {
                let message = "aggregate api request timeout".to_string();
                crate::model_router::record_route_binding_error(
                    storage,
                    model_for_log,
                    candidate.id.as_str(),
                    message.as_str(),
                );
                let request = request
                    .take()
                    .expect("request should still be available for timeout response");
                super::super::super::record_gateway_request_outcome(
                    path,
                    504,
                    Some("aggregate_api"),
                );
                super::super::super::trace_log::log_request_final(
                    trace_id,
                    504,
                    Some(key_id),
                    Some(candidate_url.as_str()),
                    Some(message.as_str()),
                    started_at.elapsed().as_millis(),
                );
                super::super::super::write_request_log(
                    storage,
                    super::super::super::request_log::RequestLogTraceContext {
                        trace_id: Some(trace_id),
                        conversation_id,
                        original_path: Some(original_path),
                        adapted_path: Some(upstream_path),
                        response_adapter: Some(candidate_response_adapter),
                        effective_service_tier: effective_service_tier_for_log,
                        aggregate_api_supplier_name: candidate_supplier_name.as_deref(),
                        aggregate_api_url: Some(candidate_url.as_str()),
                        attempted_aggregate_api_ids: Some(attempted_aggregate_api_ids.as_slice()),
                        ..Default::default()
                    },
                    Some(key_id),
                    None,
                    path,
                    request_method,
                    model_for_log,
                    reasoning_for_log,
                    Some(candidate_url.as_str()),
                    Some(504),
                    RequestLogUsage::default(),
                    Some(message.as_str()),
                    Some(started_at.elapsed().as_millis()),
                );
                respond_error(request, 504, message.as_str(), Some(trace_id));
                return Ok(());
            }

            let mut url = match build_upstream_url(candidate_url.as_str(), effective_path.as_str())
            {
                Ok(url) => url,
                Err(_) => {
                    last_attempt_url = Some(candidate_url.clone());
                    last_attempt_supplier_name = candidate_supplier_name.clone();
                    last_attempt_error = Some("invalid aggregate api url".to_string());
                    last_failure_status = 502;
                    if has_more_candidates {
                        super::super::super::record_gateway_failover_attempt();
                        continue;
                    }
                    break;
                }
            };

            match &auth_config {
                AggregateApiAuthConfig::ApiKeyQuery { name } => {
                    url = replace_query_param(url, name.as_str(), secret.trim());
                }
                AggregateApiAuthConfig::UserPassQueryPair {
                    username_name,
                    password_name,
                } => {
                    let parsed: UserPassSecret = serde_json::from_str(secret.trim())
                        .map_err(|_| "invalid aggregate api secret".to_string())?;
                    url =
                        replace_query_param(url, username_name.as_str(), parsed.username.as_str());
                    url =
                        replace_query_param(url, password_name.as_str(), parsed.password.as_str());
                }
                _ => {}
            }
            let url_for_log = redacted_url_for_log(&url);

            let buffered_sse_failover = candidate_needs_buffered_sse_failover(
                &candidate,
                upstream_is_stream,
                has_more_candidates,
            );
            let candidate_request_deadline = if buffered_sse_failover {
                Some(buffered_sse_failover_deadline(request_deadline))
            } else {
                effective_response_header_deadline(
                    &candidate,
                    upstream_path,
                    upstream_is_stream,
                    request_deadline,
                )
            };

            let builder = build_aggregate_api_request(
                &client,
                request.as_ref().expect("request should still be available"),
                method,
                url.clone(),
                &candidate_body,
                secret.as_str(),
                &auth_config,
                &injected_headers,
                candidate_request_deadline,
                upstream_is_stream,
            )?;

            let attempt_started_at = Instant::now();
            let upstream = match builder.send() {
                Ok(resp) => {
                    let duration_ms =
                        super::super::super::duration_to_millis(attempt_started_at.elapsed());
                    super::super::super::metrics::record_gateway_upstream_attempt(
                        duration_ms,
                        false,
                    );
                    resp
                }
                Err(err) => {
                    let duration_ms =
                        super::super::super::duration_to_millis(attempt_started_at.elapsed());
                    super::super::super::metrics::record_gateway_upstream_attempt(
                        duration_ms,
                        true,
                    );
                    let message = format!("aggregate api upstream error: {err}");
                    crate::model_router::record_route_binding_error(
                        storage,
                        model_for_log,
                        candidate.id.as_str(),
                        message.as_str(),
                    );
                    last_attempt_url = Some(url_for_log.clone());
                    last_attempt_supplier_name = candidate_supplier_name.clone();
                    last_attempt_error = Some(message);
                    last_failure_status = 502;
                    mark_wool_failure(storage, &candidate);
                    if !buffered_sse_failover
                        && attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL
                    {
                        continue;
                    }
                    break;
                }
            };

            if !upstream.status().is_success() {
                let status_code = upstream.status().as_u16();
                let upstream_request_id = first_upstream_header(
                    upstream.headers(),
                    &["x-request-id", "x-oai-request-id"],
                );
                let upstream_cf_ray = first_upstream_header(upstream.headers(), &["cf-ray"]);
                let upstream_auth_error =
                    first_upstream_header(upstream.headers(), &["x-openai-authorization-error"]);
                let upstream_identity_error_code =
                    crate::gateway::extract_identity_error_code_from_headers(upstream.headers());
                let upstream_body = upstream
                    .bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let message = aggregate_api_failure_message(
                    status_code,
                    upstream_body.as_ref(),
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                );
                crate::model_router::record_route_binding_error(
                    storage,
                    model_for_log,
                    candidate.id.as_str(),
                    message.as_str(),
                );
                last_attempt_url = Some(url_for_log.clone());
                last_attempt_supplier_name = candidate_supplier_name.clone();
                last_attempt_error = Some(message);
                last_failure_status = 502;
                mark_wool_failure(storage, &candidate);
                if !buffered_sse_failover && attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL
                {
                    continue;
                }
                break;
            }

            let upstream = if buffered_sse_failover {
                let upstream_status = upstream.status();
                let buffered = collect_buffered_sse_upstream(upstream);
                if buffered.terminal_seen {
                    let body = buffered.body;
                    reqwest::blocking::Response::from(
                        axum::http::Response::builder()
                            .status(upstream_status)
                            .body(body)
                            .map_err(|err| format!("build buffered response failed: {err}"))?,
                    )
                } else {
                    let reason = if buffered.read_error.is_some() {
                        "read_error_before_terminal"
                    } else {
                        "no_terminal_before_buffer_deadline"
                    };
                    let terminal_hint =
                        "received_sse_without_terminal_event expected=response.completed|response.done";
                    let event_tail = if buffered.event_tail.is_empty() {
                        "-".to_string()
                    } else {
                        buffered.event_tail.join(">")
                    };
                    let message = format!(
                        "{} [provider_buffered_sse=true aggregate_api_id={} supplier={} reason={} terminal_hint={} last_sse_event={} event_tail={} bytes={} buffer_elapsed_ms={} failover_possible={}]",
                        buffered
                            .read_error
                            .as_deref()
                            .unwrap_or("aggregate api buffered stream incomplete before downstream response"),
                        candidate.id,
                        candidate_supplier_name.as_deref().unwrap_or("-"),
                        reason,
                        terminal_hint,
                        buffered.last_sse_event.as_deref().unwrap_or("-"),
                        event_tail,
                        buffered.body.len(),
                        buffered.elapsed_ms,
                        has_more_candidates
                    );
                    crate::model_router::record_route_binding_error(
                        storage,
                        model_for_log,
                        candidate.id.as_str(),
                        message.as_str(),
                    );
                    last_attempt_url = Some(url_for_log.clone());
                    last_attempt_supplier_name = candidate_supplier_name.clone();
                    last_attempt_error = Some(message);
                    last_failure_status = 502;
                    mark_wool_failure(storage, &candidate);
                    break;
                }
            } else {
                upstream
            };

            let inflight_guard = super::super::super::acquire_account_inflight(key_id);
            let passthrough_sse_protocol = resolve_passthrough_sse_protocol(
                &candidate,
                upstream_path,
                candidate_response_adapter,
            );
            let no_upstream_after_handshake_timeout = (candidate.compatibility_mode
                && upstream_path.starts_with("/v1/responses"))
            .then_some(LOW_QUALITY_RELAY_NO_UPSTREAM_AFTER_HANDSHAKE_TIMEOUT);
            let bridge = super::super::super::respond_with_upstream(
                request
                    .take()
                    .expect("request should be available before bridge"),
                GatewayUpstreamResponse::Blocking(upstream),
                inflight_guard,
                candidate_response_adapter,
                passthrough_sse_protocol,
                None,
                path,
                None,
                is_stream,
                false,
                Some(trace_id),
                None,
                started_at,
                no_upstream_after_handshake_timeout,
            )?;
            let bridge_output_text_len = bridge
                .usage
                .output_text
                .as_deref()
                .map(str::trim)
                .map(str::len)
                .unwrap_or(0);
            super::super::super::trace_log::log_bridge_result(
                super::super::super::trace_log::BridgeResultLog {
                    trace_id,
                    adapter: format!("{candidate_response_adapter:?}").as_str(),
                    path,
                    is_stream,
                    stream_terminal_seen: bridge.stream_terminal_seen,
                    stream_terminal_error: bridge.stream_terminal_error.as_deref(),
                    delivery_error: bridge.delivery_error.as_deref(),
                    output_text_len: bridge_output_text_len,
                    output_tokens: bridge.usage.output_tokens,
                    delivered_status_code: bridge.delivered_status_code,
                    upstream_error_hint: bridge.upstream_error_hint.as_deref(),
                    upstream_request_id: bridge.upstream_request_id.as_deref(),
                    upstream_cf_ray: bridge.upstream_cf_ray.as_deref(),
                    upstream_auth_error: bridge.upstream_auth_error.as_deref(),
                    upstream_identity_error_code: bridge.upstream_identity_error_code.as_deref(),
                    upstream_content_type: bridge.upstream_content_type.as_deref(),
                    last_sse_event_type: bridge.last_sse_event_type.as_deref(),
                },
            );
            let bridge_ok = bridge.is_ok(is_stream);
            let mut final_error = bridge.upstream_error_hint.clone();
            if final_error.is_none() && !bridge_ok {
                final_error =
                    Some(bridge.error_message(is_stream).unwrap_or_else(|| {
                        "aggregate api upstream response incomplete".to_string()
                    }));
            }
            if final_error.is_some() && is_stream {
                let cause = if bridge.stream_terminal_error.is_some() {
                    "upstream_stream_failed"
                } else {
                    "response_incomplete"
                };
                let detail = format!(
                    "{} [cause={} bridge_stage=after_downstream_response_started failover_possible={} stream_terminal_seen={} last_sse_event={} upstream_content_type={} upstream_request_id={} cf_ray={}]",
                    final_error.as_deref().unwrap_or("aggregate api stream failed"),
                    cause,
                    has_more_candidates && !bridge.stream_terminal_seen,
                    bridge.stream_terminal_seen,
                    bridge.last_sse_event_type.as_deref().unwrap_or("-"),
                    bridge.upstream_content_type.as_deref().unwrap_or("-"),
                    bridge.upstream_request_id.as_deref().unwrap_or("-"),
                    bridge.upstream_cf_ray.as_deref().unwrap_or("-"),
                );
                final_error = Some(detail);
            }
            let status_code =
                bridge
                    .delivered_status_code
                    .unwrap_or_else(|| if bridge_ok { 200 } else { 502 });
            let status_code = if final_error.is_some() && status_code < 400 {
                502
            } else {
                status_code
            };
            let usage = bridge.usage;
            if final_error.is_some() || status_code >= 400 {
                mark_wool_failure(storage, &candidate);
                crate::model_router::record_route_binding_error(
                    storage,
                    model_for_log,
                    candidate.id.as_str(),
                    final_error
                        .as_deref()
                        .unwrap_or("aggregate api bridge response failed"),
                );
            } else {
                if is_wool {
                    let _ =
                        storage.mark_aggregate_api_wool_preflight_success(candidate.id.as_str());
                }
                crate::model_router::record_route_binding_success(
                    storage,
                    model_for_log,
                    candidate.id.as_str(),
                );
            }

            super::super::super::record_gateway_request_outcome(
                path,
                status_code,
                Some("aggregate_api"),
            );
            super::super::super::trace_log::log_request_final(
                trace_id,
                status_code,
                Some(key_id),
                Some(url_for_log.as_str()),
                final_error.as_deref(),
                started_at.elapsed().as_millis(),
            );
            super::super::super::write_request_log(
                storage,
                super::super::super::request_log::RequestLogTraceContext {
                    trace_id: Some(trace_id),
                    conversation_id,
                    original_path: Some(original_path),
                    adapted_path: Some(upstream_path),
                    response_adapter: Some(candidate_response_adapter),
                    effective_service_tier: effective_service_tier_for_log,
                    aggregate_api_supplier_name: candidate_supplier_name.as_deref(),
                    aggregate_api_url: Some(candidate_url.as_str()),
                    attempted_aggregate_api_ids: Some(attempted_aggregate_api_ids.as_slice()),
                    ..Default::default()
                },
                Some(key_id),
                None,
                path,
                request_method,
                model_for_log,
                reasoning_for_log,
                Some(url_for_log.as_str()),
                Some(status_code),
                RequestLogUsage {
                    input_tokens: usage.input_tokens,
                    cached_input_tokens: usage.cached_input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.total_tokens,
                    reasoning_output_tokens: usage.reasoning_output_tokens,
                    first_response_ms: usage.first_response_ms,
                },
                final_error.as_deref(),
                Some(started_at.elapsed().as_millis()),
            );
            succeeded = true;
            break;
        }

        if succeeded {
            return Ok(());
        }

        if candidate_idx + 1 < total_candidates {
            super::super::super::record_gateway_failover_attempt();
        }
    }

    // Wool re-scan: if wool candidates were skipped but nothing succeeded, retry them now.
    let has_wool_skips =
        wool_skipped_cooldown > 0 || wool_skipped_inflight > 0 || wool_skipped_preflight > 0;
    if has_wool_skips && !wool_candidates_for_rescan.is_empty() {
        for candidate in wool_candidates_for_rescan {
            if wool_candidate_in_cooldown(&candidate) {
                continue;
            }
            let Some(_wool_inflight_guard) = super::super::super::acquire_wool_inflight(
                candidate.id.as_str(),
                wool_per_api_limit(&candidate),
                super::super::super::wool_pool_max_inflight(),
            ) else {
                continue;
            };
            let candidate_supplier_name = candidate.supplier_name.clone();
            let candidate_url = candidate.url.clone();
            attempted_aggregate_api_ids.push(candidate.id.clone());
            let Some(secret) = storage
                .find_aggregate_api_secret_by_id(candidate.id.as_str())
                .map_err(|err| err.to_string())?
            else {
                continue;
            };
            if !ensure_wool_preflight(storage, &candidate, secret.as_str(), &client) {
                continue;
            }
            let candidate_contract = aggregate_candidate_adapter_contract(
                storage,
                model_for_log,
                &candidate,
                path,
                AdapterContract::native_openai_responses_passthrough(),
            );
            let upstream_path = candidate_contract.upstream_path_for(path);
            let effective_path = effective_action_path(&candidate, upstream_path);
            let candidate_response_adapter = candidate_contract.response_adapter();
            let upstream_is_stream =
                is_stream && !candidate_contract.disables_upstream_stream_passthrough();
            let candidate_body = if candidate_contract.requires_responses_to_chat_rewrite() {
                match responses_to_chat_candidate_body(body, upstream_is_stream) {
                    Ok(value) => value,
                    Err(_) => continue,
                }
            } else {
                aggregate_passthrough_candidate_body(body, path)
            };
            let upstream_model =
                crate::model_router::resolve_upstream_model_for_aggregate_candidate(
                    storage,
                    model_for_log,
                    candidate.id.as_str(),
                );
            let candidate_body =
                apply_candidate_upstream_model(candidate_body, upstream_model.as_deref());
            let candidate_body = apply_candidate_fast_service_tier(&candidate, candidate_body);
            let (auth_config, injected_headers) = match parse_auth_config(&candidate) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let mut url = match build_upstream_url(candidate_url.as_str(), effective_path.as_str())
            {
                Ok(url) => url,
                Err(_) => continue,
            };
            match &auth_config {
                AggregateApiAuthConfig::ApiKeyQuery { name } => {
                    url = replace_query_param(url, name.as_str(), secret.trim());
                }
                AggregateApiAuthConfig::UserPassQueryPair {
                    username_name,
                    password_name,
                } => {
                    let Ok(parsed) = serde_json::from_str::<UserPassSecret>(secret.trim()) else {
                        continue;
                    };
                    url =
                        replace_query_param(url, username_name.as_str(), parsed.username.as_str());
                    url =
                        replace_query_param(url, password_name.as_str(), parsed.password.as_str());
                }
                _ => {}
            }
            let url_for_log = redacted_url_for_log(&url);
            let builder = match build_aggregate_api_request(
                &client,
                request.as_ref().expect("request should still be available"),
                method,
                url.clone(),
                &candidate_body,
                secret.as_str(),
                &auth_config,
                &injected_headers,
                effective_response_header_deadline(
                    &candidate,
                    upstream_path,
                    upstream_is_stream,
                    request_deadline,
                ),
                upstream_is_stream,
            ) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let upstream = match builder.send() {
                Ok(resp) => resp,
                Err(err) => {
                    mark_wool_failure(storage, &candidate);
                    last_attempt_url = Some(url_for_log.clone());
                    last_attempt_supplier_name = candidate_supplier_name.clone();
                    last_attempt_error = Some(format!("aggregate api upstream error: {err}"));
                    last_failure_status = 502;
                    continue;
                }
            };
            if !upstream.status().is_success() {
                let status_code = upstream.status().as_u16();
                mark_wool_failure(storage, &candidate);
                last_attempt_url = Some(url_for_log.clone());
                last_attempt_supplier_name = candidate_supplier_name.clone();
                last_attempt_error = Some(format!("upstream status={status_code}"));
                last_failure_status = 502;
                continue;
            }
            let inflight_guard = super::super::super::acquire_account_inflight(key_id);
            let passthrough_sse_protocol = resolve_passthrough_sse_protocol(
                &candidate,
                upstream_path,
                candidate_response_adapter,
            );
            let no_upstream_after_handshake_timeout = (candidate.compatibility_mode
                && upstream_path.starts_with("/v1/responses"))
            .then_some(LOW_QUALITY_RELAY_NO_UPSTREAM_AFTER_HANDSHAKE_TIMEOUT);
            let bridge = super::super::super::respond_with_upstream(
                request
                    .take()
                    .expect("request should be available before bridge"),
                GatewayUpstreamResponse::Blocking(upstream),
                inflight_guard,
                candidate_response_adapter,
                passthrough_sse_protocol,
                None,
                path,
                None,
                is_stream,
                false,
                Some(trace_id),
                None,
                started_at,
                no_upstream_after_handshake_timeout,
            )?;
            let bridge_output_text_len = bridge
                .usage
                .output_text
                .as_deref()
                .map(str::trim)
                .map(str::len)
                .unwrap_or(0);
            super::super::super::trace_log::log_bridge_result(
                super::super::super::trace_log::BridgeResultLog {
                    trace_id,
                    adapter: format!("{candidate_response_adapter:?}").as_str(),
                    path,
                    is_stream,
                    stream_terminal_seen: bridge.stream_terminal_seen,
                    stream_terminal_error: bridge.stream_terminal_error.as_deref(),
                    delivery_error: bridge.delivery_error.as_deref(),
                    output_text_len: bridge_output_text_len,
                    output_tokens: bridge.usage.output_tokens,
                    delivered_status_code: bridge.delivered_status_code,
                    upstream_error_hint: bridge.upstream_error_hint.as_deref(),
                    upstream_request_id: bridge.upstream_request_id.as_deref(),
                    upstream_cf_ray: bridge.upstream_cf_ray.as_deref(),
                    upstream_auth_error: bridge.upstream_auth_error.as_deref(),
                    upstream_identity_error_code: bridge.upstream_identity_error_code.as_deref(),
                    upstream_content_type: bridge.upstream_content_type.as_deref(),
                    last_sse_event_type: bridge.last_sse_event_type.as_deref(),
                },
            );
            let bridge_ok = bridge.is_ok(is_stream);
            let mut final_error = bridge.upstream_error_hint.clone();
            if final_error.is_none() && !bridge_ok {
                final_error =
                    Some(bridge.error_message(is_stream).unwrap_or_else(|| {
                        "aggregate api upstream response incomplete".to_string()
                    }));
            }
            let status_code =
                bridge
                    .delivered_status_code
                    .unwrap_or_else(|| if bridge_ok { 200 } else { 502 });
            let status_code = if final_error.is_some() && status_code < 400 {
                502
            } else {
                status_code
            };
            let usage = bridge.usage;
            if final_error.is_some() || status_code >= 400 {
                mark_wool_failure(storage, &candidate);
            } else {
                let _ = storage.mark_aggregate_api_wool_preflight_success(candidate.id.as_str());
                crate::model_router::record_route_binding_success(
                    storage,
                    model_for_log,
                    candidate.id.as_str(),
                );
            }
            super::super::super::record_gateway_request_outcome(
                path,
                status_code,
                Some("aggregate_api"),
            );
            super::super::super::trace_log::log_request_final(
                trace_id,
                status_code,
                Some(key_id),
                Some(url_for_log.as_str()),
                final_error.as_deref(),
                started_at.elapsed().as_millis(),
            );
            super::super::super::write_request_log(
                storage,
                super::super::super::request_log::RequestLogTraceContext {
                    trace_id: Some(trace_id),
                    conversation_id,
                    original_path: Some(original_path),
                    adapted_path: Some(upstream_path),
                    response_adapter: Some(candidate_response_adapter),
                    effective_service_tier: effective_service_tier_for_log,
                    aggregate_api_supplier_name: candidate_supplier_name.as_deref(),
                    aggregate_api_url: Some(candidate_url.as_str()),
                    attempted_aggregate_api_ids: Some(attempted_aggregate_api_ids.as_slice()),
                    ..Default::default()
                },
                Some(key_id),
                None,
                path,
                request_method,
                model_for_log,
                reasoning_for_log,
                Some(url_for_log.as_str()),
                Some(status_code),
                RequestLogUsage {
                    input_tokens: usage.input_tokens,
                    cached_input_tokens: usage.cached_input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.total_tokens,
                    reasoning_output_tokens: usage.reasoning_output_tokens,
                    first_response_ms: usage.first_response_ms,
                },
                final_error.as_deref(),
                Some(started_at.elapsed().as_millis()),
            );
            return Ok(());
        }
    }

    let base_message =
        last_attempt_error.unwrap_or_else(|| "aggregate api upstream response failed".to_string());
    let message = if has_wool_skips || primary_attempted > 0 {
        format!(
            "{} [wool_skip_cd={} wool_skip_inflight={} wool_skip_preflight={} primary_attempted={}]",
            base_message, wool_skipped_cooldown, wool_skipped_inflight, wool_skipped_preflight, primary_attempted,
        )
    } else {
        base_message
    };
    let status_code = last_failure_status;
    let request = request
        .take()
        .expect("request should still be available for failure response");
    super::super::super::record_gateway_request_outcome(path, status_code, Some("aggregate_api"));
    super::super::super::trace_log::log_request_final(
        trace_id,
        status_code,
        Some(key_id),
        last_attempt_url.as_deref(),
        Some(message.as_str()),
        started_at.elapsed().as_millis(),
    );
    super::super::super::write_request_log(
        storage,
        super::super::super::request_log::RequestLogTraceContext {
            trace_id: Some(trace_id),
            conversation_id,
            original_path: Some(original_path),
            adapted_path: Some(path),
            response_adapter: Some(response_adapter),
            effective_service_tier: effective_service_tier_for_log,
            aggregate_api_supplier_name: last_attempt_supplier_name.as_deref(),
            aggregate_api_url: last_attempt_url.as_deref(),
            attempted_aggregate_api_ids: Some(attempted_aggregate_api_ids.as_slice()),
            ..Default::default()
        },
        Some(key_id),
        None,
        path,
        request_method,
        model_for_log,
        reasoning_for_log,
        last_attempt_url.as_deref(),
        Some(status_code),
        RequestLogUsage::default(),
        Some(message.as_str()),
        Some(started_at.elapsed().as_millis()),
    );
    respond_error(request, status_code, message.as_str(), Some(trace_id));
    Ok(())
}

#[cfg(test)]
mod bridge_tests {
    use super::*;

    /// 函数 `candidate`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - id: 参数 id
    /// - sort: 参数 sort
    ///
    /// # 返回
    /// 返回函数执行结果
    fn candidate(id: &str, sort: i64) -> AggregateApi {
        AggregateApi {
            id: id.to_string(),
            provider_type: AGGREGATE_API_PROVIDER_CODEX.to_string(),
            supplier_name: None,
            sort,
            url: format!("https://{id}.example.com"),
            auth_type: AGGREGATE_API_AUTH_APIKEY.to_string(),
            auth_params_json: None,
            action: None,
            pool: "primary".to_string(),
            wool_max_inflight: None,
            wool_cooldown_until: None,
            wool_failure_count: 0,
            wool_last_preflight_at: None,
            fast: false,
            compatibility_mode: false,
            status: "active".to_string(),
            created_at: sort,
            updated_at: sort,
            last_test_at: None,
            last_test_status: None,
            last_test_error: None,
        }
    }

    /// 函数 `ids`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - items: 参数 items
    ///
    /// # 返回
    /// 返回函数执行结果
    fn ids(items: &[AggregateApi]) -> Vec<String> {
        items.iter().map(|item| item.id.clone()).collect()
    }

    /// 函数 `balanced_route_strategy_rotates_aggregate_candidates`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn balanced_route_strategy_rotates_aggregate_candidates() {
        let _guard = crate::test_env_guard();
        let previous = std::env::var("CODEXMANAGER_ROUTE_STRATEGY").ok();
        std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", "balanced");
        crate::gateway::reload_runtime_config_from_env();

        let mut candidates = vec![
            candidate("agg-a", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut candidates,
            "gk-aggregate-route-strategy",
            Some("gpt-5.4-mini"),
            None,
        );
        assert_eq!(ids(&candidates), vec!["agg-a", "agg-b", "agg-c"]);

        let mut second = vec![
            candidate("agg-a", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut second,
            "gk-aggregate-route-strategy",
            Some("gpt-5.4-mini"),
            None,
        );
        assert_eq!(ids(&second), vec!["agg-b", "agg-c", "agg-a"]);

        if let Some(value) = previous {
            std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", value);
        } else {
            std::env::remove_var("CODEXMANAGER_ROUTE_STRATEGY");
        }
        crate::gateway::reload_runtime_config_from_env();
    }

    #[test]
    fn aggregate_stream_requests_override_forwarded_accept_header() {
        let injected_headers = HashSet::new();

        assert!(should_skip_forward_header_for_aggregate_request(
            "Accept",
            &injected_headers,
            true,
        ));
        assert!(!should_skip_forward_header_for_aggregate_request(
            "Accept",
            &injected_headers,
            false,
        ));
        assert!(should_skip_forward_header_for_aggregate_request(
            "Accept-Encoding",
            &injected_headers,
            true,
        ));
        assert!(should_skip_forward_header_for_aggregate_request(
            "Accept-Encoding",
            &injected_headers,
            false,
        ));
    }

    /// 函数 `balanced_route_strategy_preserves_explicit_preferred_aggregate_api`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn balanced_route_strategy_preserves_explicit_preferred_aggregate_api() {
        let _guard = crate::test_env_guard();
        let previous = std::env::var("CODEXMANAGER_ROUTE_STRATEGY").ok();
        std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", "balanced");
        crate::gateway::reload_runtime_config_from_env();

        let mut candidates = vec![
            candidate("agg-preferred", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut candidates,
            "gk-aggregate-route-strategy-preferred",
            Some("gpt-5.4-mini"),
            Some("agg-preferred"),
        );
        assert_eq!(ids(&candidates), vec!["agg-preferred", "agg-b", "agg-c"]);

        let mut second = vec![
            candidate("agg-preferred", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut second,
            "gk-aggregate-route-strategy-preferred",
            Some("gpt-5.4-mini"),
            Some("agg-preferred"),
        );
        assert_eq!(ids(&second), vec!["agg-preferred", "agg-c", "agg-b"]);

        if let Some(value) = previous {
            std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", value);
        } else {
            std::env::remove_var("CODEXMANAGER_ROUTE_STRATEGY");
        }
        crate::gateway::reload_runtime_config_from_env();
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use codexmanager_core::storage::{now_ts, AggregateApi, RequestLog, Storage};

    use super::{
        aggregate_candidate_adapter_contract, aggregate_passthrough_candidate_body,
        aggregate_provider_contract_for_protocol, apply_candidate_fast_service_tier,
        build_upstream_url, effective_action_path, parse_auth_config,
        resolve_aggregate_api_rotation_candidates, resolve_passthrough_sse_protocol,
        AggregateApiAuthConfig, BUFFERED_SSE_FAILOVER_MAX_BUFFER_MS,
    };
    use crate::aggregate_api::{
        AGGREGATE_API_AUTH_APIKEY, AGGREGATE_API_PROVIDER_AZURE_OPENAI,
        AGGREGATE_API_PROVIDER_CLAUDE, AGGREGATE_API_PROVIDER_CODEX, AGGREGATE_API_PROVIDER_GEMINI,
    };
    use crate::gateway::protocol_adapter::{AdapterContract, AdapterContractKind, ProviderFamily};
    use crate::gateway::{PassthroughSseProtocol, ResponseAdapter};

    fn aggregate_api_with_action(action: Option<&str>) -> AggregateApi {
        AggregateApi {
            id: "agg-path-test".to_string(),
            provider_type: "claude".to_string(),
            supplier_name: Some("test".to_string()),
            sort: 0,
            url: "https://open.bigmodel.cn/api/anthropic".to_string(),
            auth_type: "apikey".to_string(),
            auth_params_json: None,
            action: action.map(str::to_string),
            pool: "primary".to_string(),
            wool_max_inflight: None,
            wool_cooldown_until: None,
            wool_failure_count: 0,
            wool_last_preflight_at: None,
            fast: false,
            compatibility_mode: false,
            status: "active".to_string(),
            created_at: 0,
            updated_at: 0,
            last_test_at: None,
            last_test_status: None,
            last_test_error: None,
        }
    }

    fn aggregate_api_with_provider(provider_type: &str) -> AggregateApi {
        let mut api = aggregate_api_with_action(None);
        api.provider_type = provider_type.to_string();
        api
    }

    fn aggregate_api_ids(items: &[AggregateApi]) -> Vec<String> {
        items.iter().map(|item| item.id.clone()).collect()
    }

    #[test]
    fn freemodel_streaming_responses_do_not_use_buffered_failover_guard_by_default() {
        let mut api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        api.url = "https://api.freemodel.dev/v1".to_string();

        assert!(!super::candidate_needs_buffered_sse_failover(
            &api, true, true
        ));
        assert!(!super::candidate_needs_buffered_sse_failover(
            &api, false, true
        ));
        assert!(!super::candidate_needs_buffered_sse_failover(
            &api, true, false
        ));
    }

    #[test]
    fn compatibility_mode_streaming_responses_do_not_use_buffered_failover_guard() {
        let mut api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        api.url = "https://api.example.com/v1".to_string();
        api.compatibility_mode = true;

        assert!(!super::candidate_needs_buffered_sse_failover(
            &api, true, true
        ));
    }

    #[test]
    fn fast_streaming_responses_keep_failover_possible_when_terminal_missing() {
        let mut api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        api.url = "https://api.freemodel.dev/v1".to_string();
        api.fast = true;

        let has_more_candidates = true;
        let stream_terminal_seen = false;
        let failover_possible = has_more_candidates && !stream_terminal_seen;

        assert!(failover_possible);
    }

    #[test]
    fn buffered_sse_failover_deadline_caps_long_request_deadline() {
        let long_deadline = std::time::Instant::now() + std::time::Duration::from_secs(500);
        let deadline = super::buffered_sse_failover_deadline(Some(long_deadline));

        assert!(
            deadline
                <= std::time::Instant::now()
                    + std::time::Duration::from_millis(BUFFERED_SSE_FAILOVER_MAX_BUFFER_MS + 250)
        );
    }

    #[test]
    fn compatibility_mode_caps_response_header_deadline_for_native_responses_stream() {
        let mut api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        api.compatibility_mode = true;

        let deadline = super::effective_response_header_deadline(&api, "/v1/responses", true, None)
            .expect("compatibility deadline");

        assert!(
            deadline
                <= std::time::Instant::now()
                    + super::LOW_QUALITY_RELAY_RESPONSE_HEADER_TIMEOUT
                    + std::time::Duration::from_millis(250)
        );
    }

    #[test]
    fn empty_custom_action_falls_back_to_original_path() {
        let api = aggregate_api_with_action(Some(""));
        let path = effective_action_path(&api, "/v1/messages?beta=true");
        assert_eq!(path, "/v1/messages?beta=true");
    }

    #[test]
    fn custom_action_preserves_original_query_when_action_has_none() {
        let api =
            aggregate_api_with_action(Some("/openai/deployments/gpt-4o-mini/chat/completions"));
        let path = effective_action_path(&api, "/v1/chat/completions?api-version=2024-10-21");
        assert_eq!(
            path,
            "/openai/deployments/gpt-4o-mini/chat/completions?api-version=2024-10-21"
        );
    }

    #[test]
    fn custom_action_keeps_own_query_for_azure_api_version() {
        let api = aggregate_api_with_action(Some(
            "/openai/deployments/gpt-4o-mini/chat/completions?api-version=2024-10-21",
        ));
        let path = effective_action_path(&api, "/v1/chat/completions?stream=true");
        assert_eq!(
            path,
            "/openai/deployments/gpt-4o-mini/chat/completions?api-version=2024-10-21"
        );
    }

    #[test]
    fn aggregate_responses_passthrough_strips_encrypted_content() {
        let body = Bytes::from(
            r#"{
                "model":"gpt-5.5",
                "input":[{"role":"user","content":[{"type":"input_text","text":"hi"}]}],
                "reasoning":{"encrypted_content":"gAAA_secret","effort":"low"},
                "nested":{"encrypted_content":"gAAA_nested"}
            }"#,
        );

        let rewritten = aggregate_passthrough_candidate_body(&body, "/v1/responses");
        let text = std::str::from_utf8(rewritten.as_ref()).expect("utf8");

        assert!(!text.contains("encrypted_content"));
        assert!(text.contains("gpt-5.5"));
        assert!(text.contains("effort"));
    }

    #[test]
    fn fast_candidate_injects_service_tier_when_missing() {
        let mut api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        api.fast = true;
        let body = apply_candidate_fast_service_tier(
            &api,
            Bytes::from(r#"{"model":"gpt-5.5","input":"ping"}"#),
        );
        let value: serde_json::Value = serde_json::from_slice(body.as_ref()).expect("json");

        assert_eq!(value["service_tier"], "fast");
    }

    #[test]
    fn fast_candidate_preserves_explicit_service_tier() {
        let mut api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        api.fast = true;
        let body = apply_candidate_fast_service_tier(
            &api,
            Bytes::from(r#"{"model":"gpt-5.5","input":"ping","service_tier":"default"}"#),
        );
        let value: serde_json::Value = serde_json::from_slice(body.as_ref()).expect("json");

        assert_eq!(value["service_tier"], "default");
    }

    #[test]
    fn azure_openai_defaults_to_raw_api_key_header() {
        let api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_AZURE_OPENAI);
        let (auth, injected_headers) = parse_auth_config(&api).expect("parse auth");
        match auth {
            AggregateApiAuthConfig::ApiKeyHeader { name, format } => {
                assert_eq!(name, "api-key");
                assert_eq!(format, "raw");
            }
            other => panic!("unexpected auth config: {other:?}"),
        }
        assert!(injected_headers.contains("api-key"));
    }

    #[test]
    fn azure_openai_is_available_for_codex_provider_rotation() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = now_ts();
        let mut api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_AZURE_OPENAI);
        api.id = "agg-azure".to_string();
        api.created_at = now;
        api.updated_at = now;
        storage
            .insert_aggregate_api(&api)
            .expect("insert azure api");

        let candidates = resolve_aggregate_api_rotation_candidates(&storage, "openai_compat", None)
            .expect("resolve candidates");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, "agg-azure");
    }

    #[test]
    fn openai_protocol_uses_native_responses_provider_contract() {
        let contract = aggregate_provider_contract_for_protocol("openai_compat");

        assert_eq!(contract.provider_family, ProviderFamily::OpenAI);
        assert_eq!(
            contract.default_contract.kind,
            AdapterContractKind::NativeResponsesPassthrough
        );
    }

    #[test]
    fn aggregate_candidate_contract_uses_responses_from_chat_when_model_router_requires_it() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = now_ts();
        let api = AggregateApi {
            id: "agg-mimo".to_string(),
            provider_type: AGGREGATE_API_PROVIDER_CODEX.to_string(),
            supplier_name: Some("mimo".to_string()),
            sort: 0,
            url: "https://mimo.example.com".to_string(),
            auth_type: AGGREGATE_API_AUTH_APIKEY.to_string(),
            auth_params_json: None,
            action: None,
            pool: "primary".to_string(),
            wool_max_inflight: None,
            wool_cooldown_until: None,
            wool_failure_count: 0,
            wool_last_preflight_at: None,
            fast: false,
            compatibility_mode: false,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_test_at: None,
            last_test_status: None,
            last_test_error: None,
        };
        storage
            .insert_aggregate_api(&api)
            .expect("insert aggregate api");
        storage
            .upsert_upstream_model_capability(
                &codexmanager_core::storage::UpstreamModelCapability {
                    id: "cap-mimo".to_string(),
                    aggregate_api_id: api.id.clone(),
                    model: "mimo-v2.5-pro".to_string(),
                    supports_responses: false,
                    supports_chat_completions: true,
                    requires_adapter: true,
                    probe_status: "success".to_string(),
                    last_error: None,
                    last_probe_at: Some(now),
                    updated_at: now,
                },
            )
            .expect("insert capability");

        let contract = aggregate_candidate_adapter_contract(
            &storage,
            Some("mimo-v2.5-pro"),
            &api,
            "/v1/responses",
            aggregate_provider_contract_for_protocol("openai_compat").default_contract,
        );

        assert_eq!(contract.provider_family, ProviderFamily::OpenAI);
        assert_eq!(contract.kind, AdapterContractKind::ResponsesFromChat);
        assert_eq!(
            contract.response_adapter(),
            ResponseAdapter::ResponsesFromChatCompletions
        );
        assert_eq!(
            contract.upstream_path_for("/v1/responses"),
            "/v1/chat/completions"
        );
    }

    #[test]
    fn compatibility_mode_keeps_native_responses_without_recent_passthrough_failure() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let mut api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        api.id = "agg-free-healthy".to_string();
        api.url = "https://api.freemodel.dev/v1".to_string();
        api.compatibility_mode = true;

        let contract = aggregate_candidate_adapter_contract(
            &storage,
            Some("gpt-5.5"),
            &api,
            "/v1/responses",
            aggregate_provider_contract_for_protocol("openai_compat").default_contract,
        );

        assert_eq!(
            contract.kind,
            AdapterContractKind::NativeResponsesPassthrough
        );
        assert_eq!(contract.response_adapter(), ResponseAdapter::Passthrough);
    }

    #[test]
    fn compatibility_mode_recent_passthrough_failure_uses_streaming_chat_fallback() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = now_ts();
        let mut api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        api.id = "agg-free-recent-failure".to_string();
        api.url = "https://api.freemodel.dev/v1".to_string();
        api.compatibility_mode = true;
        storage
            .insert_request_log(&RequestLog {
                request_path: "/v1/responses".to_string(),
                original_path: Some("/v1/responses".to_string()),
                adapted_path: Some("/v1/responses".to_string()),
                method: "POST".to_string(),
                model: Some("gpt-5.5".to_string()),
                response_adapter: Some("Passthrough".to_string()),
                upstream_url: Some("https://api.freemodel.dev/v1/responses".to_string()),
                aggregate_api_url: Some("https://api.freemodel.dev".to_string()),
                status_code: Some(502),
                error: Some(
                    "上游首帧等待超时 [bridge_stage=after_downstream_response_started stream_terminal_seen=false last_sse_event=response.failed]"
                        .to_string(),
                ),
                created_at: now,
                ..Default::default()
            })
            .expect("insert request log");

        let contract = aggregate_candidate_adapter_contract(
            &storage,
            Some("gpt-5.5"),
            &api,
            "/v1/responses",
            aggregate_provider_contract_for_protocol("openai_compat").default_contract,
        );

        assert_eq!(
            contract.kind,
            AdapterContractKind::ResponsesFromStreamingChat
        );
        assert_eq!(
            contract.response_adapter(),
            ResponseAdapter::ResponsesFromChatCompletions
        );
        assert_eq!(
            contract.upstream_path_for("/v1/responses"),
            "/v1/chat/completions"
        );
        assert!(!contract.disables_upstream_stream_passthrough());
    }

    #[test]
    fn compatibility_mode_recent_passthrough_success_restores_native_responses() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = now_ts();
        let mut api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        api.id = "agg-free-restored".to_string();
        api.url = "https://api.freemodel.dev/v1".to_string();
        api.compatibility_mode = true;
        storage
            .insert_request_log(&RequestLog {
                request_path: "/v1/responses".to_string(),
                original_path: Some("/v1/responses".to_string()),
                adapted_path: Some("/v1/responses".to_string()),
                method: "POST".to_string(),
                model: Some("gpt-5.5".to_string()),
                response_adapter: Some("Passthrough".to_string()),
                upstream_url: Some("https://api.freemodel.dev/v1/responses".to_string()),
                aggregate_api_url: Some("https://api.freemodel.dev".to_string()),
                status_code: Some(502),
                error: Some(
                    "connection reset [bridge_stage=after_downstream_response_started stream_terminal_seen=false]"
                        .to_string(),
                ),
                created_at: now - 1,
                ..Default::default()
            })
            .expect("insert failure log");
        storage
            .insert_request_log(&RequestLog {
                request_path: "/v1/responses".to_string(),
                original_path: Some("/v1/responses".to_string()),
                adapted_path: Some("/v1/responses".to_string()),
                method: "POST".to_string(),
                model: Some("gpt-5.5".to_string()),
                response_adapter: Some("Passthrough".to_string()),
                upstream_url: Some("https://api.freemodel.dev/v1/responses".to_string()),
                aggregate_api_url: Some("https://api.freemodel.dev".to_string()),
                status_code: Some(200),
                error: None,
                created_at: now,
                ..Default::default()
            })
            .expect("insert success log");

        let contract = aggregate_candidate_adapter_contract(
            &storage,
            Some("gpt-5.5"),
            &api,
            "/v1/responses",
            aggregate_provider_contract_for_protocol("openai_compat").default_contract,
        );

        assert_eq!(
            contract.kind,
            AdapterContractKind::NativeResponsesPassthrough
        );
        assert_eq!(contract.response_adapter(), ResponseAdapter::Passthrough);
    }

    #[test]
    fn recent_passthrough_failure_does_not_switch_non_compatibility_candidate() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = now_ts();
        let mut api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        api.id = "agg-free-non-compat".to_string();
        api.url = "https://api.freemodel.dev/v1".to_string();
        api.compatibility_mode = false;
        storage
            .insert_request_log(&RequestLog {
                request_path: "/v1/responses".to_string(),
                original_path: Some("/v1/responses".to_string()),
                adapted_path: Some("/v1/responses".to_string()),
                method: "POST".to_string(),
                model: Some("gpt-5.5".to_string()),
                response_adapter: Some("Passthrough".to_string()),
                upstream_url: Some("https://api.freemodel.dev/v1/responses".to_string()),
                aggregate_api_url: Some("https://api.freemodel.dev/v1".to_string()),
                status_code: Some(502),
                error: Some(
                    "connection reset [bridge_stage=after_downstream_response_started stream_terminal_seen=false]"
                        .to_string(),
                ),
                created_at: now,
                ..Default::default()
            })
            .expect("insert request log");

        let contract = aggregate_candidate_adapter_contract(
            &storage,
            Some("gpt-5.5"),
            &api,
            "/v1/responses",
            aggregate_provider_contract_for_protocol("openai_compat").default_contract,
        );

        assert_eq!(
            contract.kind,
            AdapterContractKind::NativeResponsesPassthrough
        );
        assert_eq!(contract.response_adapter(), ResponseAdapter::Passthrough);
    }

    #[test]
    fn anthropic_protocol_contract_stays_anthropic_native() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let api = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CLAUDE);

        let contract = aggregate_candidate_adapter_contract(
            &storage,
            Some("claude-sonnet"),
            &api,
            "/v1/messages",
            aggregate_provider_contract_for_protocol("anthropic_native").default_contract,
        );

        assert_eq!(contract.provider_family, ProviderFamily::Anthropic);
        assert_eq!(contract.kind, AdapterContractKind::AnthropicNative);
    }

    #[test]
    fn disabled_preferred_aggregate_api_is_not_reinserted_into_rotation() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = now_ts();
        let mut disabled = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        disabled.id = "agg-disabled".to_string();
        disabled.status = "disabled".to_string();
        disabled.created_at = now;
        disabled.updated_at = now;
        let mut active = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_AZURE_OPENAI);
        active.id = "agg-azure".to_string();
        active.created_at = now + 1;
        active.updated_at = now + 1;
        storage
            .insert_aggregate_api(&disabled)
            .expect("insert disabled api");
        storage
            .insert_aggregate_api(&active)
            .expect("insert active api");

        let candidates = resolve_aggregate_api_rotation_candidates(
            &storage,
            "openai_compat",
            Some("agg-disabled"),
        )
        .expect("resolve candidates");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, "agg-azure");
    }

    #[test]
    fn aggregate_rotation_rebuilds_from_current_active_status_and_sort() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = now_ts();
        let mut first = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        first.id = "agg-first".to_string();
        first.sort = 0;
        first.created_at = now;
        first.updated_at = now;
        let mut second = aggregate_api_with_provider(AGGREGATE_API_PROVIDER_CODEX);
        second.id = "agg-second".to_string();
        second.sort = 1;
        second.created_at = now + 1;
        second.updated_at = now + 1;
        storage.insert_aggregate_api(&first).expect("insert first");
        storage
            .insert_aggregate_api(&second)
            .expect("insert second");

        let initial = resolve_aggregate_api_rotation_candidates(&storage, "openai_compat", None)
            .expect("resolve initial");
        assert_eq!(aggregate_api_ids(&initial), vec!["agg-first", "agg-second"]);

        storage
            .update_aggregate_api_status("agg-first", "disabled")
            .expect("disable first");
        let after_disable =
            resolve_aggregate_api_rotation_candidates(&storage, "openai_compat", None)
                .expect("resolve after disable");
        assert_eq!(aggregate_api_ids(&after_disable), vec!["agg-second"]);

        storage
            .update_aggregate_api_status("agg-first", "active")
            .expect("reenable first");
        storage
            .update_aggregate_api_sort("agg-first", 2)
            .expect("move first after second");
        let after_reenable =
            resolve_aggregate_api_rotation_candidates(&storage, "openai_compat", None)
                .expect("resolve after reenable");
        assert_eq!(
            aggregate_api_ids(&after_reenable),
            vec!["agg-second", "agg-first"]
        );
    }

    #[test]
    fn claude_messages_passthrough_uses_anthropic_native_terminal_rules() {
        let api = aggregate_api_with_action(None);
        let protocol = resolve_passthrough_sse_protocol(
            &api,
            "/v1/messages?beta=true",
            ResponseAdapter::Passthrough,
        );
        assert_eq!(protocol, Some(PassthroughSseProtocol::AnthropicNative));
    }

    #[test]
    fn build_upstream_url_preserves_base_path_prefix() {
        let url = build_upstream_url(
            "https://open.bigmodel.cn/api/anthropic",
            "/v1/messages?beta=true",
        )
        .expect("build upstream url");
        assert_eq!(
            url.as_str(),
            "https://open.bigmodel.cn/api/anthropic/v1/messages?beta=true"
        );
    }

    #[test]
    fn build_upstream_url_keeps_root_base_behavior() {
        let url = build_upstream_url("https://api.example.com", "/v1/messages?beta=true")
            .expect("build upstream url");
        assert_eq!(
            url.as_str(),
            "https://api.example.com/v1/messages?beta=true"
        );
    }

    #[test]
    fn build_upstream_url_avoids_double_v1_prefix() {
        let url = build_upstream_url(
            "https://api.example.com/v1",
            "/v1/chat/completions?stream=true",
        )
        .expect("build upstream url");
        assert_eq!(
            url.as_str(),
            "https://api.example.com/v1/chat/completions?stream=true"
        );
    }

    #[test]
    fn redacted_url_for_log_masks_sensitive_query_values() {
        let url = build_upstream_url("https://api.example.com/v1", "/responses")
            .map(|url| super::replace_query_param(url, "key", "sk-secret"))
            .expect("build url");
        let redacted = super::redacted_url_for_log(&url);

        assert!(redacted.contains("key=%3Credacted%3E"));
        assert!(!redacted.contains("sk-secret"));
    }

    #[test]
    fn responses_from_chat_adapter_disables_upstream_stream_passthrough() {
        let contract = AdapterContract::responses_from_chat(ProviderFamily::OpenAI);
        assert!(contract.disables_upstream_stream_passthrough());
        assert!(contract.requires_responses_to_chat_rewrite());
        assert_eq!(
            contract.upstream_path_for("/v1/responses"),
            "/v1/chat/completions"
        );
    }

    #[test]
    fn responses_from_streaming_chat_adapter_keeps_upstream_stream() {
        let contract = AdapterContract::responses_from_streaming_chat(ProviderFamily::OpenAI);
        assert!(!contract.disables_upstream_stream_passthrough());
        assert!(contract.requires_responses_to_chat_rewrite());
        assert_eq!(
            contract.upstream_path_for("/v1/responses"),
            "/v1/chat/completions"
        );
    }

    #[test]
    fn gemini_native_candidates_resolve_to_gemini_provider_only() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = now_ts();
        for (id, provider_type) in [
            ("agg-codex", AGGREGATE_API_PROVIDER_CODEX),
            ("agg-claude", AGGREGATE_API_PROVIDER_CLAUDE),
            ("agg-gemini", AGGREGATE_API_PROVIDER_GEMINI),
        ] {
            storage
                .insert_aggregate_api(&AggregateApi {
                    id: id.to_string(),
                    provider_type: provider_type.to_string(),
                    supplier_name: Some(id.to_string()),
                    sort: 0,
                    url: format!("https://{id}.example.com"),
                    auth_type: AGGREGATE_API_AUTH_APIKEY.to_string(),
                    auth_params_json: None,
                    action: None,
                    pool: "primary".to_string(),
                    wool_max_inflight: None,
                    wool_cooldown_until: None,
                    wool_failure_count: 0,
                    wool_last_preflight_at: None,
                    fast: false,
                    compatibility_mode: false,
                    status: "active".to_string(),
                    created_at: now,
                    updated_at: now,
                    last_test_at: None,
                    last_test_status: None,
                    last_test_error: None,
                })
                .expect("insert aggregate api");
        }

        let candidates = resolve_aggregate_api_rotation_candidates(&storage, "gemini_native", None)
            .expect("resolve gemini candidates");
        let candidate_ids = candidates
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(candidate_ids, vec!["agg-gemini"]);
    }

    /// 函数 `final_error_promotes_success_status_to_bad_gateway`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn final_error_promotes_success_status_to_bad_gateway() {
        let status_code = bridge_status_code(Some(200), true, Some("unsupported model"));
        assert_eq!(status_code, 502);
    }

    /// 函数 `successful_bridge_keeps_success_status`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn successful_bridge_keeps_success_status() {
        let status_code = bridge_status_code(Some(200), true, None);
        assert_eq!(status_code, 200);
    }

    /// 函数 `incomplete_bridge_without_status_defaults_to_bad_gateway`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn incomplete_bridge_without_status_defaults_to_bad_gateway() {
        let status_code = bridge_status_code(None, false, None);
        assert_eq!(status_code, 502);
    }

    /// 函数 `bridge_status_code`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - delivered_status_code: 参数 delivered_status_code
    /// - bridge_ok: 参数 bridge_ok
    /// - final_error: 参数 final_error
    ///
    /// # 返回
    /// 返回函数执行结果
    fn bridge_status_code(
        delivered_status_code: Option<u16>,
        bridge_ok: bool,
        final_error: Option<&str>,
    ) -> u16 {
        let status_code =
            delivered_status_code.unwrap_or_else(|| if bridge_ok { 200 } else { 502 });
        if final_error.is_some() && status_code < 400 {
            502
        } else {
            status_code
        }
    }
}
