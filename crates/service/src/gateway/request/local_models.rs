use codexmanager_core::rpc::types::{ModelInfo, ModelsResponse};

const MODEL_CACHE_SCOPE_DEFAULT: &str = "default";
const DEFAULT_CODEX_COMPAT_INSTRUCTIONS: &str =
    "You are Codex, a helpful AI assistant. Follow the user's instructions.";

fn default_codex_model_messages() -> serde_json::Value {
    serde_json::json!({
        "instructions_template": "You are Codex, a coding agent based on GPT-5. You and the user share one workspace, and your job is to collaborate with them until their goal is genuinely handled.\n\n{{personality}}\n",
        "instructions_variables": {
            "personality_default": "",
            "personality_friendly": "# Personality\n\nYou optimize for team morale and being a supportive teammate as much as code quality.",
            "personality_pragmatic": "# Personality\n\nYou are a deeply pragmatic, effective software engineer. You take engineering quality seriously."
        },
        "prefer_websockets": true,
    })
}

#[derive(serde::Serialize)]
struct OfficialModelsResponse {
    models: Vec<serde_json::Value>,
}

fn normalize_visibility(value: Option<&str>) -> Option<&str> {
    let trimmed = value.map(str::trim).unwrap_or_default();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn is_visible_for_codex_app(model: &ModelInfo) -> bool {
    if !model.supported_in_api {
        return false;
    }
    !matches!(
        normalize_visibility(model.visibility.as_deref()),
        Some("hide") | Some("hidden")
    )
}

fn serialize_visible_model_for_codex_app(model: &ModelInfo) -> Option<serde_json::Value> {
    if !is_visible_for_codex_app(model) {
        return None;
    }

    let mut value = serde_json::to_value(model).ok()?;
    let object = value.as_object_mut()?;

    if model.display_name.trim().is_empty() {
        object.insert(
            "display_name".to_string(),
            serde_json::Value::String(model.slug.clone()),
        );
    }
    if normalize_visibility(model.visibility.as_deref()).is_none() {
        object.insert(
            "visibility".to_string(),
            serde_json::Value::String("list".to_string()),
        );
    }
    if model
        .shell_type
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        object.insert(
            "shell_type".to_string(),
            serde_json::Value::String("shell_command".to_string()),
        );
    }
    if model.supported_reasoning_levels.is_empty() {
        object.insert(
            "supported_reasoning_levels".to_string(),
            serde_json::Value::Array(Vec::new()),
        );
    }
    if model.additional_speed_tiers.is_empty() {
        object.insert(
            "additional_speed_tiers".to_string(),
            serde_json::Value::Array(Vec::new()),
        );
    }
    if model
        .base_instructions
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        object.insert(
            "base_instructions".to_string(),
            serde_json::Value::String(DEFAULT_CODEX_COMPAT_INSTRUCTIONS.to_string()),
        );
    }
    if model.model_messages.is_none() {
        object.insert("model_messages".to_string(), default_codex_model_messages());
    }
    if model.supports_reasoning_summaries.is_none() {
        object.insert(
            "supports_reasoning_summaries".to_string(),
            serde_json::Value::Bool(false),
        );
    }
    if model.default_reasoning_summary.is_none() {
        object.insert(
            "default_reasoning_summary".to_string(),
            serde_json::Value::String("auto".to_string()),
        );
    }
    if model.support_verbosity.is_none() {
        object.insert(
            "support_verbosity".to_string(),
            serde_json::Value::Bool(false),
        );
    }
    if model
        .web_search_tool_type
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        object.insert(
            "web_search_tool_type".to_string(),
            serde_json::Value::String("text".to_string()),
        );
    }
    if model.truncation_policy.is_none() {
        object.insert(
            "truncation_policy".to_string(),
            serde_json::json!({ "mode": "tokens", "limit": 10000 }),
        );
    }
    if model.supports_parallel_tool_calls.is_none() {
        object.insert(
            "supports_parallel_tool_calls".to_string(),
            serde_json::Value::Bool(false),
        );
    }
    if model.effective_context_window_percent.is_none() {
        object.insert(
            "effective_context_window_percent".to_string(),
            serde_json::Value::Number(serde_json::Number::from(95)),
        );
    }
    if model.experimental_supported_tools.is_empty() {
        object.insert(
            "experimental_supported_tools".to_string(),
            serde_json::Value::Array(Vec::new()),
        );
    }
    if model.input_modalities.is_empty() {
        object.insert(
            "input_modalities".to_string(),
            serde_json::json!(["text", "image"]),
        );
    }
    if model.supports_search_tool.is_none() {
        object.insert(
            "supports_search_tool".to_string(),
            serde_json::Value::Bool(false),
        );
    }

    Some(value)
}

fn serialize_models_response(models: &ModelsResponse) -> String {
    let models = crate::apikey_models::ensure_codex_image_tool_model_listed(models);
    let visible_models = models
        .models
        .iter()
        .filter_map(serialize_visible_model_for_codex_app)
        .collect::<Vec<_>>();
    serde_json::to_string(&OfficialModelsResponse {
        models: visible_models,
    })
    .unwrap_or_else(|_| "{\"models\":[]}".to_string())
}

fn models_etag_header(models: &ModelsResponse) -> Result<Option<tiny_http::Header>, String> {
    let Some(etag) = models.extra.get("etag").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let header = tiny_http::Header::from_bytes(b"etag".as_slice(), etag.as_bytes())
        .map_err(|_| "build etag header failed".to_string())?;
    Ok(Some(header))
}

/// 函数 `read_cached_models_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-12
///
/// # 参数
/// - storage: 参数 storage
///
/// # 返回
/// 返回函数执行结果
fn read_cached_models_response(
    storage: &codexmanager_core::storage::Storage,
) -> Result<ModelsResponse, String> {
    crate::apikey_models::read_model_options_from_storage(storage)
}

/// 函数 `maybe_respond_local_models`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn maybe_respond_local_models(
    request: tiny_http::Request,
    trace_id: &str,
    key_id: &str,
    protocol_type: &str,
    original_path: &str,
    path: &str,
    response_adapter: super::ResponseAdapter,
    request_method: &str,
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    storage: &codexmanager_core::storage::Storage,
) -> Result<Option<tiny_http::Request>, String> {
    let is_models_list = request_method.eq_ignore_ascii_case("GET")
        && (path == "/v1/models" || path.starts_with("/v1/models?"));
    if !is_models_list {
        return Ok(Some(request));
    }
    let context = super::local_response::LocalResponseContext {
        trace_id,
        key_id,
        protocol_type,
        original_path,
        path,
        response_adapter,
        request_method,
        model_for_log,
        reasoning_for_log,
        storage,
    };
    let cached = match read_cached_models_response(storage) {
        Ok(models) => models,
        Err(err) => {
            let message = crate::gateway::bilingual_error(
                "读取模型缓存失败",
                format!("model options cache read failed: {err}"),
            );
            super::local_response::respond_local_terminal_error(request, &context, 503, message)?;
            return Ok(None);
        }
    };

    let models = if !cached.is_empty() {
        cached
    } else {
        match crate::apikey_models::read_model_options(true) {
            Ok(fetched) if !fetched.is_empty() => {
                if let Err(err) =
                    crate::apikey_models::save_model_options_with_storage(storage, &fetched)
                {
                    log::warn!(
                        "event=gateway_model_catalog_upsert_failed scope={} err={}",
                        MODEL_CACHE_SCOPE_DEFAULT,
                        err
                    );
                }
                fetched
            }
            Ok(_) => {
                let message = crate::gateway::bilingual_error(
                    "模型刷新后返回空目录",
                    "models refresh returned empty catalog",
                );
                super::local_response::respond_local_terminal_error(
                    request, &context, 503, message,
                )?;
                return Ok(None);
            }
            Err(err) => {
                let message = crate::gateway::bilingual_error(
                    "模型刷新失败",
                    format!("models refresh failed: {err}"),
                );
                super::local_response::respond_local_terminal_error(
                    request, &context, 503, message,
                )?;
                return Ok(None);
            }
        }
    };

    let output_models = crate::apikey_models::ensure_codex_image_tool_model_listed(&models);
    let output = serialize_models_response(&output_models);
    let extra_headers = models_etag_header(&output_models)?.into_iter().collect();
    super::local_response::respond_local_json_with_headers(
        request,
        &context,
        output,
        super::request_log::RequestLogUsage::default(),
        extra_headers,
    )?;
    Ok(None)
}

#[cfg(test)]
#[path = "tests/local_models_tests.rs"]
mod tests;
