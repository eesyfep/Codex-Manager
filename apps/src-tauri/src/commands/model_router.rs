use crate::commands::shared::rpc_call_in_background;

#[tauri::command]
pub async fn service_model_router_session_list(
    addr: Option<String>,
    workspace: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "workspace": workspace });
    rpc_call_in_background("modelRouter/session/list", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_session_update_model(
    addr: Option<String>,
    thread_id: String,
    model: String,
    reasoning_effort: Option<String>,
    source: Option<String>,
    locked: Option<bool>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "threadId": thread_id,
        "model": model,
        "reasoningEffort": reasoning_effort,
        "source": source,
        "locked": locked,
    });
    rpc_call_in_background("modelRouter/session/updateModel", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_session_apply_latest_for_workspace(
    addr: Option<String>,
    workspace: String,
    model: String,
    reasoning_effort: Option<String>,
    source: Option<String>,
    locked: Option<bool>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "workspace": workspace,
        "model": model,
        "reasoningEffort": reasoning_effort,
        "source": source,
        "locked": locked,
    });
    rpc_call_in_background("modelRouter/session/applyLatestForWorkspace", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_session_set_subagent_model(
    addr: Option<String>,
    parent_thread_id: String,
    model: String,
    reasoning_effort: Option<String>,
    source: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "parentThreadId": parent_thread_id,
        "model": model,
        "reasoningEffort": reasoning_effort,
        "source": source,
    });
    rpc_call_in_background("modelRouter/session/subagentModel/set", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_session_clear_subagent_model(
    addr: Option<String>,
    parent_thread_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "parentThreadId": parent_thread_id,
    });
    rpc_call_in_background("modelRouter/session/subagentModel/clear", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_workspace_default_set(
    addr: Option<String>,
    workspace: String,
    default_model: Option<String>,
    default_reasoning_effort: Option<String>,
    inherit_last_session: Option<bool>,
    auto_remember: Option<bool>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "workspace": workspace,
        "defaultModel": default_model,
        "defaultReasoningEffort": default_reasoning_effort,
        "inheritLastSession": inherit_last_session,
        "autoRemember": auto_remember,
    });
    rpc_call_in_background("modelRouter/workspaceDefault/set", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_workspace_default_delete(
    addr: Option<String>,
    workspace: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "workspace": workspace,
    });
    rpc_call_in_background("modelRouter/workspaceDefault/delete", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_binding_list(
    addr: Option<String>,
    model: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "model": model });
    rpc_call_in_background("modelRouter/binding/list", addr, Some(params)).await
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn service_model_router_binding_save(
    addr: Option<String>,
    id: Option<String>,
    model: String,
    aggregate_api_id: String,
    enabled: Option<bool>,
    priority: Option<i64>,
    weight: Option<i64>,
    route_strategy: Option<String>,
    manual_preferred: Option<bool>,
    supports_responses: Option<bool>,
    supports_chat_completions: Option<bool>,
    requires_adapter: Option<bool>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "id": id,
        "model": model,
        "aggregateApiId": aggregate_api_id,
        "enabled": enabled,
        "priority": priority,
        "weight": weight,
        "routeStrategy": route_strategy,
        "manualPreferred": manual_preferred,
        "supportsResponses": supports_responses,
        "supportsChatCompletions": supports_chat_completions,
        "requiresAdapter": requires_adapter,
    });
    rpc_call_in_background("modelRouter/binding/save", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_binding_delete(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "id": id });
    rpc_call_in_background("modelRouter/binding/delete", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_probe_run(
    addr: Option<String>,
    aggregate_api_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "aggregateApiId": aggregate_api_id });
    rpc_call_in_background("modelRouter/probe/run", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_probe_run_all(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("modelRouter/probe/runAll", addr, None).await
}

#[tauri::command]
pub async fn service_model_router_probe_manual_model(
    addr: Option<String>,
    aggregate_api_id: String,
    model: String,
    supports_responses: Option<bool>,
    supports_chat_completions: Option<bool>,
    requires_adapter: Option<bool>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "aggregateApiId": aggregate_api_id,
        "model": model,
        "supportsResponses": supports_responses,
        "supportsChatCompletions": supports_chat_completions,
        "requiresAdapter": requires_adapter,
    });
    rpc_call_in_background("modelRouter/probe/manualModel", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_probe_quick_call(
    addr: Option<String>,
    aggregate_api_id: String,
    model: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "aggregateApiId": aggregate_api_id,
        "model": model,
    });
    rpc_call_in_background("modelRouter/probe/quickCall", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_probe_apply(
    addr: Option<String>,
    probe_run_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "probeRunId": probe_run_id });
    rpc_call_in_background("modelRouter/probe/apply", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_probe_list(
    addr: Option<String>,
    limit: Option<i64>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "limit": limit });
    rpc_call_in_background("modelRouter/probe/list", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_router_import_codexmanager(
    addr: Option<String>,
    source_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "sourcePath": source_path });
    rpc_call_in_background("modelRouter/import/codexManager", addr, Some(params)).await
}
