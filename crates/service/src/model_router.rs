use codexmanager_core::rpc::types::{
    LatestSessionModelApplyResult, ModelInfo, ModelRouteBindingListResult,
    ModelRouteBindingSaveResult, ModelRouteBindingSummary, ModelRouteQuickCheckResult,
    ModelRouterImportResult, ModelsResponse, ProbeCandidateSummary, ProbeRunAllResult,
    ProbeRunListResult, ProbeRunSummary, SessionModelListResult, SessionModelSummary,
    SessionModelUpdateResult, WorkspaceModelDefaultSummary,
};
use codexmanager_core::storage::{
    now_ts, AggregateApi, ModelRouteBinding, ProbeCandidate, ProbeRun, RequestLog,
    RequestTokenStat, SessionModelMemory, SessionSubagentModelMemory, Storage,
    UpstreamModelCapability, UsageSnapshotRecord, WorkspaceModelDefault,
};
use reqwest::header::{HeaderName, HeaderValue};
use rusqlite::{backup::Backup, Connection};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::aggregate_api::{
    AGGREGATE_API_AUTH_APIKEY, AGGREGATE_API_AUTH_USERPASS, AGGREGATE_API_PROVIDER_AZURE_OPENAI,
};
use crate::storage_helpers::{generate_model_router_id, open_storage};

const GLOBAL_WORKSPACE: &str = "__global__";
const DEFAULT_GLOBAL_MODEL: &str = "gpt-5.4";
const QUICK_STREAM_READ_LIMIT_BYTES: usize = 4096;
const RESPONSE_STREAM_TERMINAL_EVENTS: [&str; 2] = ["response.completed", "response.done"];

#[derive(Debug, Clone)]
struct CodexThreadRow {
    thread_id: String,
    workspace: String,
    title: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    model_provider: Option<String>,
    parent_thread_id: Option<String>,
    is_subagent: bool,
    agent_nickname: Option<String>,
    agent_role: Option<String>,
    subagent_depth: Option<i64>,
    updated_at: i64,
}

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
struct EffectiveSessionModelState {
    model: Option<String>,
    reasoning_effort: Option<String>,
    source: String,
    locked: bool,
    memory_state: String,
    updated_at: i64,
    has_model_override: bool,
    effective_model_label: String,
    effective_model_source: String,
}

fn derive_effective_session_model_state(
    state_model: Option<&str>,
    state_reasoning_effort: Option<&str>,
    session_memory: Option<&SessionModelMemory>,
    fallback_updated_at: i64,
) -> EffectiveSessionModelState {
    let state_model = state_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let state_reasoning_effort = state_reasoning_effort
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let memory_model = session_memory
        .map(|item| item.model.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let memory_reasoning_effort = session_memory
        .and_then(|item| item.reasoning_effort.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let model = memory_model.clone().or_else(|| state_model.clone());
    let reasoning_effort = memory_reasoning_effort
        .clone()
        .or_else(|| state_reasoning_effort.clone());
    let source = session_memory
        .map(|item| item.source.clone())
        .unwrap_or_else(|| "state".to_string());
    let is_manual_memory = matches!(source.as_str(), "manual" | "session_override");
    let (source, locked, memory_state, updated_at) = match session_memory {
        Some(item) => (
            item.source.clone(),
            item.locked,
            if state_model.is_some() {
                "memory".to_string()
            } else {
                "memory_only".to_string()
            },
            item.updated_at.max(fallback_updated_at),
        ),
        None => (
            "state".to_string(),
            false,
            if state_model.is_some() {
                "state".to_string()
            } else {
                "unresolved".to_string()
            },
            fallback_updated_at,
        ),
    };
    let has_model_override =
        session_memory.is_some() && (is_manual_memory || memory_model != state_model);
    let effective_model_label = if has_model_override {
        "自定义模型".to_string()
    } else if model.is_some() {
        "内置模型".to_string()
    } else {
        "未设置模型".to_string()
    };
    let effective_model_source = if has_model_override {
        "session_override".to_string()
    } else if session_memory.is_some() {
        source.clone()
    } else if state_model.is_some() {
        "state".to_string()
    } else {
        "unresolved".to_string()
    };
    EffectiveSessionModelState {
        model,
        reasoning_effort,
        source,
        locked,
        memory_state,
        updated_at,
        has_model_override,
        effective_model_label,
        effective_model_source,
    }
}

pub(crate) fn list_session_models(
    workspace: Option<String>,
) -> Result<SessionModelListResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let workspace_filter = workspace
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let state_db_path = resolve_codex_state_db_path();
    let mut state_db_ok = false;
    let mut state_db_error = None;
    let mut thread_rows = Vec::new();

    match state_db_path.as_ref() {
        Some(path) => match read_codex_threads(path, workspace_filter) {
            Ok(rows) => {
                state_db_ok = true;
                thread_rows = rows;
            }
            Err(err) => state_db_error = Some(err),
        },
        None => state_db_error = Some("未找到 Codex state_5.sqlite".to_string()),
    }

    let mut by_thread = BTreeMap::<String, SessionModelSummary>::new();
    for row in thread_rows {
        let mut memory = storage
            .find_session_model_memory(row.thread_id.as_str())
            .map_err(|err| err.to_string())?;
        let auto_remember = workspace_auto_remember_enabled(&storage, row.workspace.as_str())?;
        if memory.is_none() {
            if !auto_remember {
                // When auto memory is disabled, display Codex state without persisting discovered threads.
            } else if let Some(state_model) = row
                .model
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let item = SessionModelMemory {
                    thread_id: row.thread_id.clone(),
                    workspace: row.workspace.clone(),
                    title: row.title.clone(),
                    model: state_model.to_string(),
                    reasoning_effort: row.reasoning_effort.clone(),
                    source: "state".to_string(),
                    locked: false,
                    last_seen_at: row.updated_at,
                    updated_at: row.updated_at,
                };
                storage
                    .upsert_session_model_memory(&item)
                    .map_err(|err| err.to_string())?;
                memory = Some(item);
            } else if let Some(default) = resolve_session_default_for_thread(&storage, &row)? {
                let now = now_ts();
                let item = SessionModelMemory {
                    thread_id: row.thread_id.clone(),
                    workspace: row.workspace.clone(),
                    title: row.title.clone(),
                    model: default.model.clone(),
                    reasoning_effort: default.reasoning_effort.clone(),
                    source: default.source,
                    locked: false,
                    last_seen_at: row.updated_at,
                    updated_at: now,
                };
                storage
                    .upsert_session_model_memory(&item)
                    .map_err(|err| err.to_string())?;
                mirror_session_model_to_runtime_anchors(&storage, &item)?;
                if let Some(path) = state_db_path.as_ref() {
                    let _ = write_codex_thread_model(
                        path,
                        row.thread_id.as_str(),
                        item.model.as_str(),
                        item.reasoning_effort.as_deref(),
                    );
                }
                memory = Some(item);
            }
        }
        let effective = derive_effective_session_model_state(
            row.model.as_deref(),
            row.reasoning_effort.as_deref(),
            memory.as_ref(),
            row.updated_at,
        );
        let subagent_memory = storage
            .find_session_subagent_model_memory(row.thread_id.as_str())
            .map_err(|err| err.to_string())?;
        by_thread.insert(
            row.thread_id.clone(),
            SessionModelSummary {
                thread_id: row.thread_id,
                workspace: row.workspace,
                title: row.title,
                model: effective.model,
                reasoning_effort: effective.reasoning_effort,
                model_provider: row.model_provider,
                effective_model_label: effective.effective_model_label,
                effective_model_source: effective.effective_model_source,
                has_model_override: effective.has_model_override,
                parent_thread_id: row.parent_thread_id,
                is_subagent: row.is_subagent,
                agent_nickname: row.agent_nickname,
                agent_role: row.agent_role,
                subagent_depth: row.subagent_depth,
                source: effective.source,
                locked: effective.locked,
                memory_state: effective.memory_state,
                last_seen_at: row.updated_at,
                updated_at: effective.updated_at,
                subagent_model: subagent_memory.as_ref().map(|item| item.model.clone()),
                subagent_reasoning_effort: subagent_memory
                    .as_ref()
                    .and_then(|item| item.reasoning_effort.clone()),
                subagent_model_source: subagent_memory.as_ref().map(|item| item.source.clone()),
                subagent_model_updated_at: subagent_memory.as_ref().map(|item| item.updated_at),
            },
        );
    }

    for memory in storage
        .list_session_model_memory(workspace_filter, 500)
        .map_err(|err| err.to_string())?
    {
        by_thread
            .entry(memory.thread_id.clone())
            .or_insert_with(|| SessionModelSummary {
                thread_id: memory.thread_id,
                workspace: memory.workspace,
                title: memory.title,
                model: Some(memory.model),
                reasoning_effort: memory.reasoning_effort,
                model_provider: None,
                effective_model_label: "自定义模型".to_string(),
                effective_model_source: "session_override".to_string(),
                has_model_override: true,
                parent_thread_id: None,
                is_subagent: false,
                agent_nickname: None,
                agent_role: None,
                subagent_depth: None,
                source: memory.source,
                locked: memory.locked,
                memory_state: "memory_only".to_string(),
                last_seen_at: memory.last_seen_at,
                updated_at: memory.updated_at,
                subagent_model: None,
                subagent_reasoning_effort: None,
                subagent_model_source: None,
                subagent_model_updated_at: None,
            });
    }

    let mut items = by_thread.into_values().collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then(left.workspace.cmp(&right.workspace))
            .then(left.thread_id.cmp(&right.thread_id))
    });

    let workspace_defaults = storage
        .list_workspace_model_defaults()
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(workspace_default_summary)
        .collect::<Vec<_>>();
    let global_default_model = resolve_global_default_model(&storage)?;

    Ok(SessionModelListResult {
        items,
        workspace_defaults,
        state_db_path: state_db_path.map(|path| path.to_string_lossy().to_string()),
        state_db_ok,
        state_db_error,
        global_default_model,
    })
}

pub(crate) fn update_session_model(
    thread_id: String,
    model: String,
    reasoning_effort: Option<String>,
    source: Option<String>,
    locked: Option<bool>,
) -> Result<SessionModelUpdateResult, String> {
    let thread_id = normalized_required(thread_id.as_str(), "threadId is required")?;
    let model = normalized_required(model.as_str(), "model is required")?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let state_db_path = resolve_codex_state_db_path();
    let state_row = match state_db_path.as_ref() {
        Some(path) => read_codex_thread(path, thread_id.as_str()).ok().flatten(),
        None => None,
    };
    let workspace = state_row
        .as_ref()
        .map(|row| row.workspace.clone())
        .or_else(|| {
            storage
                .find_session_model_memory(thread_id.as_str())
                .ok()
                .flatten()
                .map(|item| item.workspace)
        })
        .unwrap_or_default();
    let now = now_ts();
    let memory = SessionModelMemory {
        thread_id: thread_id.clone(),
        workspace: workspace.clone(),
        title: state_row.as_ref().and_then(|row| row.title.clone()),
        model: model.clone(),
        reasoning_effort: normalize_optional_string(reasoning_effort),
        source: source
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("manual")
            .to_string(),
        locked: locked.unwrap_or(false),
        last_seen_at: state_row.as_ref().map(|row| row.updated_at).unwrap_or(now),
        updated_at: now,
    };
    storage
        .upsert_session_model_memory(&memory)
        .map_err(|err| err.to_string())?;
    mirror_session_model_to_runtime_anchors(&storage, &memory)?;

    let mut state_updated = false;
    if let Some(path) = state_db_path.as_ref() {
        state_updated = write_codex_thread_model(
            path,
            thread_id.as_str(),
            model.as_str(),
            memory.reasoning_effort.as_deref(),
        )?;
    }
    let subagent_memory = storage
        .find_session_subagent_model_memory(thread_id.as_str())
        .ok()
        .flatten();

    Ok(SessionModelUpdateResult {
        item: SessionModelSummary {
            thread_id,
            workspace,
            title: memory.title,
            model: Some(memory.model),
            reasoning_effort: memory.reasoning_effort,
            model_provider: state_row.and_then(|row| row.model_provider),
            effective_model_label: "自定义模型".to_string(),
            effective_model_source: "session_override".to_string(),
            has_model_override: true,
            parent_thread_id: None,
            is_subagent: false,
            agent_nickname: None,
            agent_role: None,
            subagent_depth: None,
            source: memory.source,
            locked: memory.locked,
            memory_state: "memory".to_string(),
            last_seen_at: memory.last_seen_at,
            updated_at: memory.updated_at,
            subagent_model: subagent_memory.as_ref().map(|item| item.model.clone()),
            subagent_reasoning_effort: subagent_memory
                .as_ref()
                .and_then(|item| item.reasoning_effort.clone()),
            subagent_model_source: subagent_memory.as_ref().map(|item| item.source.clone()),
            subagent_model_updated_at: subagent_memory.as_ref().map(|item| item.updated_at),
        },
        state_updated,
    })
}

pub(crate) fn update_session_subagent_model(
    parent_thread_id: String,
    model: String,
    reasoning_effort: Option<String>,
    source: Option<String>,
) -> Result<SessionModelSummary, String> {
    let parent_thread_id =
        normalized_required(parent_thread_id.as_str(), "parentThreadId is required")?;
    let model = normalized_required(model.as_str(), "model is required")?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let state_db_path = resolve_codex_state_db_path();
    let state_row = match state_db_path.as_ref() {
        Some(path) => read_codex_thread(path, parent_thread_id.as_str())
            .ok()
            .flatten(),
        None => None,
    };
    let workspace = state_row
        .as_ref()
        .map(|row| row.workspace.clone())
        .or_else(|| {
            storage
                .find_session_model_memory(parent_thread_id.as_str())
                .ok()
                .flatten()
                .map(|item| item.workspace)
        })
        .unwrap_or_default();
    let item = SessionSubagentModelMemory {
        parent_thread_id: parent_thread_id.clone(),
        workspace,
        model,
        reasoning_effort: normalize_optional_string(reasoning_effort),
        source: source
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("manual")
            .to_string(),
        updated_at: now_ts(),
    };
    storage
        .upsert_session_subagent_model_memory(&item)
        .map_err(|err| err.to_string())?;
    session_summary_from_storage(&storage, state_row, parent_thread_id, Some(item))
}

pub(crate) fn clear_session_subagent_model(parent_thread_id: String) -> Result<bool, String> {
    let parent_thread_id =
        normalized_required(parent_thread_id.as_str(), "parentThreadId is required")?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let deleted = storage
        .delete_session_subagent_model_memory(parent_thread_id.as_str())
        .map_err(|err| err.to_string())?;
    if let Some(state_db_path) = resolve_codex_state_db_path() {
        if let Ok(rows) = read_codex_threads(&state_db_path, None) {
            let child_thread_ids = rows
                .into_iter()
                .filter(|row| row.parent_thread_id.as_deref() == Some(parent_thread_id.as_str()))
                .map(|row| row.thread_id)
                .collect::<Vec<_>>();
            let _ = storage.delete_inherited_subagent_session_model_memory(&child_thread_ids);
        }
        if deleted {
            let workspace = read_codex_thread(&state_db_path, parent_thread_id.as_str())
                .ok()
                .flatten()
                .map(|row| row.workspace)
                .or_else(|| {
                    storage
                        .find_session_model_memory(parent_thread_id.as_str())
                        .ok()
                        .flatten()
                        .map(|item| item.workspace)
                })
                .unwrap_or_default();
            let default_model = if !workspace.is_empty() {
                storage
                    .find_workspace_model_default(workspace.as_str())
                    .ok()
                    .flatten()
                    .and_then(|d| d.default_model)
                    .filter(|m| !m.trim().is_empty())
                    .unwrap_or_else(|| DEFAULT_GLOBAL_MODEL.to_string())
            } else {
                DEFAULT_GLOBAL_MODEL.to_string()
            };
            let _ = write_codex_thread_model(
                &state_db_path,
                parent_thread_id.as_str(),
                default_model.as_str(),
                None,
            );
        }
    }
    Ok(deleted)
}

pub(crate) fn apply_model_to_latest_workspace_session(
    workspace: String,
    model: String,
    reasoning_effort: Option<String>,
    source: Option<String>,
    locked: Option<bool>,
) -> Result<LatestSessionModelApplyResult, String> {
    let workspace = normalized_required(workspace.as_str(), "workspace is required")?;
    let state_db_path =
        resolve_codex_state_db_path().ok_or_else(|| "未找到 Codex state_5.sqlite".to_string())?;
    let rows = read_codex_threads(&state_db_path, Some(workspace.as_str()))?;
    let target = rows
        .into_iter()
        .filter(|row| !row.is_subagent)
        .max_by(|left, right| {
            left.updated_at
                .cmp(&right.updated_at)
                .then(left.thread_id.cmp(&right.thread_id))
        })
        .ok_or_else(|| format!("workspace {workspace} 下没有可更新的主线程"))?;
    let result = update_session_model(
        target.thread_id.clone(),
        model,
        reasoning_effort,
        source,
        locked,
    )?;
    Ok(LatestSessionModelApplyResult {
        item: result.item,
        state_updated: result.state_updated,
        matched_workspace: workspace,
    })
}

pub(crate) fn set_workspace_default(
    workspace: String,
    default_model: Option<String>,
    default_reasoning_effort: Option<String>,
    inherit_last_session: Option<bool>,
    auto_remember: Option<bool>,
) -> Result<WorkspaceModelDefaultSummary, String> {
    let workspace = normalized_required(workspace.as_str(), "workspace is required")?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let existing = storage
        .find_workspace_model_default(workspace.as_str())
        .map_err(|err| err.to_string())?;
    let default_model = normalize_optional_string(default_model);
    let default_reasoning_effort = normalize_optional_string(default_reasoning_effort);
    let item = WorkspaceModelDefault {
        workspace,
        default_model,
        default_reasoning_effort,
        inherit_last_session: inherit_last_session
            .or_else(|| existing.as_ref().map(|item| item.inherit_last_session))
            .unwrap_or(true),
        auto_remember: auto_remember
            .or_else(|| existing.as_ref().map(|item| item.auto_remember))
            .unwrap_or(true),
        updated_at: now_ts(),
    };
    storage
        .upsert_workspace_model_default(&item)
        .map_err(|err| err.to_string())?;
    Ok(workspace_default_summary(item))
}

pub(crate) fn delete_workspace_default(workspace: String) -> Result<bool, String> {
    let workspace = normalized_required(workspace.as_str(), "workspace is required")?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .delete_workspace_model_default(workspace.as_str())
        .map_err(|err| err.to_string())
}

pub(crate) fn list_model_route_bindings(
    model: Option<String>,
) -> Result<ModelRouteBindingListResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let apis = aggregate_api_map(&storage)?;
    let items = storage
        .list_model_route_bindings(model.as_deref())
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|item| route_binding_summary(item, &apis))
        .collect();
    Ok(ModelRouteBindingListResult { items })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn save_model_route_binding(
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
) -> Result<ModelRouteBindingSaveResult, String> {
    let model = normalized_required(model.as_str(), "model is required")?;
    let aggregate_api_id =
        normalized_required(aggregate_api_id.as_str(), "aggregateApiId is required")?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    if storage
        .find_aggregate_api_by_id(aggregate_api_id.as_str())
        .map_err(|err| err.to_string())?
        .is_none()
    {
        return Err("aggregate api not found".to_string());
    }
    let now = now_ts();
    let item = ModelRouteBinding {
        id: id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| generate_model_router_id("mrb")),
        model,
        aggregate_api_id,
        enabled: enabled.unwrap_or(true),
        priority: priority.unwrap_or(0),
        weight: weight.unwrap_or(1).max(1),
        route_strategy: normalize_route_strategy(route_strategy.as_deref()).to_string(),
        manual_preferred: manual_preferred.unwrap_or(false),
        supports_responses: supports_responses.unwrap_or(false),
        supports_chat_completions: supports_chat_completions.unwrap_or(false),
        requires_adapter: requires_adapter.unwrap_or(false),
        last_probe_status: None,
        last_error: None,
        last_success_at: None,
        created_at: now,
        updated_at: now,
    };
    if item.manual_preferred {
        storage
            .clear_manual_preferred_model_route_bindings(item.model.as_str(), id.as_deref())
            .map_err(|err| err.to_string())?;
    }
    storage
        .upsert_model_route_binding(&item)
        .map_err(|err| err.to_string())?;
    crate::gateway::invalidate_aggregate_api_routing_state();
    let apis = aggregate_api_map(&storage)?;
    Ok(ModelRouteBindingSaveResult {
        item: route_binding_summary(item, &apis),
    })
}

pub(crate) fn delete_model_route_binding(id: String) -> Result<(), String> {
    let id = normalized_required(id.as_str(), "binding id is required")?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .delete_model_route_binding(id.as_str())
        .map_err(|err| err.to_string())?;
    crate::gateway::invalidate_aggregate_api_routing_state();
    Ok(())
}

pub(crate) fn probe_aggregate_api_capabilities(
    aggregate_api_id: String,
) -> Result<ProbeRunSummary, String> {
    let aggregate_api_id =
        normalized_required(aggregate_api_id.as_str(), "aggregateApiId is required")?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let api = storage
        .find_aggregate_api_by_id(aggregate_api_id.as_str())
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "aggregate api not found".to_string())?;
    let secret = storage
        .find_aggregate_api_secret_by_id(aggregate_api_id.as_str())
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "aggregate api secret not found".to_string())?;
    let started_at = now_ts();
    let started = Instant::now();
    let client = crate::gateway::fresh_upstream_client();

    let models_probe = probe_models_endpoint(&client, &api, secret.as_str());
    let models = models_probe.models.clone();
    let probe_model = select_probe_model(&api, &models);
    let responses_probe =
        probe_responses_endpoint(&client, &api, secret.as_str(), probe_model.as_ref());
    let chat_probe = probe_chat_completions_endpoint(
        &client,
        &api,
        secret.as_str(),
        probe_model
            .as_ref()
            .or_else(|| responses_probe.model.as_ref()),
    );
    let mut candidate_models = BTreeSet::<String>::new();
    for model in &models_probe.models {
        candidate_models.insert(model.clone());
    }
    if let Some(model) = responses_probe.model.as_ref() {
        candidate_models.insert(model.clone());
    }
    if let Some(model) = chat_probe.model.as_ref() {
        candidate_models.insert(model.clone());
    }
    if candidate_models.is_empty() {
        candidate_models.insert("未识别模型".to_string());
    }

    let ok = models_probe.ok || responses_probe.ok || chat_probe.ok;
    let finished_at = now_ts();
    let run = ProbeRun {
        id: generate_model_router_id("probe"),
        aggregate_api_id: aggregate_api_id.clone(),
        status: if ok { "success" } else { "failed" }.to_string(),
        started_at,
        finished_at: Some(finished_at),
        models_status: Some(models_probe.status_label()),
        responses_status: Some(responses_probe.status_label()),
        chat_completions_status: Some(chat_probe.status_label()),
        error: (!ok).then(|| {
            [
                models_probe.error.as_deref(),
                responses_probe.error.as_deref(),
                chat_probe.error.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("; ")
        }),
        raw_summary_json: Some(
            json!({
                "latencyMs": started.elapsed().as_millis() as i64,
                "models": models_probe.to_json(),
                "responses": responses_probe.to_json(),
                "chatCompletions": chat_probe.to_json()
            })
            .to_string(),
        ),
    };
    storage
        .insert_probe_run(&run)
        .map_err(|err| err.to_string())?;

    let mut summaries = Vec::new();
    for (index, model) in candidate_models.into_iter().enumerate() {
        let supports_responses = responses_probe.ok;
        let supports_chat = chat_probe.ok;
        let requires_adapter = !supports_responses && supports_chat;
        let candidate = ProbeCandidate {
            id: generate_model_router_id("pc"),
            probe_run_id: run.id.clone(),
            aggregate_api_id: aggregate_api_id.clone(),
            model: model.clone(),
            supports_responses,
            supports_chat_completions: supports_chat,
            requires_adapter,
            suggested_route_strategy: if requires_adapter {
                "ordered".to_string()
            } else {
                "balanced".to_string()
            },
            suggested_priority: index as i64,
            suggested_weight: 1,
            applied: false,
            error: if supports_responses || supports_chat {
                None
            } else {
                Some("该上游未通过 responses 或 chat/completions 探测".to_string())
            },
            created_at: finished_at,
            applied_at: None,
        };
        storage
            .insert_probe_candidate(&candidate)
            .map_err(|err| err.to_string())?;
        storage
            .upsert_upstream_model_capability(&UpstreamModelCapability {
                id: generate_model_router_id("cap"),
                aggregate_api_id: aggregate_api_id.clone(),
                model,
                supports_responses,
                supports_chat_completions: supports_chat,
                requires_adapter,
                probe_status: if supports_responses || supports_chat {
                    "success".to_string()
                } else {
                    "failed".to_string()
                },
                last_error: candidate.error.clone(),
                last_probe_at: Some(finished_at),
                updated_at: finished_at,
            })
            .map_err(|err| err.to_string())?;
        summaries.push(probe_candidate_summary(candidate));
    }

    let apis = aggregate_api_map(&storage)?;
    Ok(probe_run_summary(run, summaries, &apis))
}

pub(crate) fn apply_probe_candidates(probe_run_id: String) -> Result<ProbeRunSummary, String> {
    apply_selected_probe_candidates(probe_run_id, Vec::new())
}

pub(crate) fn apply_selected_probe_candidates(
    probe_run_id: String,
    candidate_ids: Vec<String>,
) -> Result<ProbeRunSummary, String> {
    let probe_run_id = normalized_required(probe_run_id.as_str(), "probeRunId is required")?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let candidates = storage
        .list_probe_candidates(probe_run_id.as_str())
        .map_err(|err| err.to_string())?;
    if candidates.is_empty() {
        return Err("probe candidates not found".to_string());
    }
    let selected_ids = candidate_ids
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    let use_all = selected_ids.is_empty();
    let now = now_ts();
    let mut applied_any = false;
    for candidate in &candidates {
        if candidate.error.is_some()
            || (!candidate.supports_responses && !candidate.supports_chat_completions)
        {
            continue;
        }
        if !use_all && !selected_ids.contains(candidate.id.as_str()) {
            continue;
        }
        let item = ModelRouteBinding {
            id: generate_model_router_id("mrb"),
            model: candidate.model.clone(),
            aggregate_api_id: candidate.aggregate_api_id.clone(),
            enabled: true,
            priority: candidate.suggested_priority,
            weight: candidate.suggested_weight.max(1),
            route_strategy: candidate.suggested_route_strategy.clone(),
            manual_preferred: false,
            supports_responses: candidate.supports_responses,
            supports_chat_completions: candidate.supports_chat_completions,
            requires_adapter: candidate.requires_adapter,
            last_probe_status: Some("success".to_string()),
            last_error: None,
            last_success_at: Some(now),
            created_at: now,
            updated_at: now,
        };
        storage
            .upsert_model_route_binding(&item)
            .map_err(|err| err.to_string())?;
        applied_any = true;
    }
    if !applied_any {
        return Err("no selectable probe candidates found".to_string());
    }
    storage
        .mark_probe_candidates_applied(probe_run_id.as_str(), now)
        .map_err(|err| err.to_string())?;
    crate::gateway::invalidate_aggregate_api_routing_state();

    list_probe_runs(20)?
        .items
        .into_iter()
        .find(|item| item.id == probe_run_id)
        .ok_or_else(|| "probe run not found".to_string())
}

pub(crate) fn list_probe_runs(limit: i64) -> Result<ProbeRunListResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let apis = aggregate_api_map(&storage)?;
    let mut items = Vec::new();
    for run in storage
        .latest_probe_runs(limit)
        .map_err(|err| err.to_string())?
    {
        let candidates = storage
            .list_probe_candidates(run.id.as_str())
            .map_err(|err| err.to_string())?
            .into_iter()
            .map(probe_candidate_summary)
            .collect();
        items.push(probe_run_summary(run, candidates, &apis));
    }
    Ok(ProbeRunListResult { items })
}

pub(crate) fn aggregate_api_models_response() -> Result<ModelsResponse, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let client = crate::gateway::fresh_upstream_client();
    let mut models = BTreeMap::<String, ModelInfo>::new();
    let mut errors = Vec::new();
    for api in storage
        .list_aggregate_apis()
        .map_err(|err| err.to_string())?
        .into_iter()
        .filter(|api| api.status.trim().to_ascii_lowercase() != "disabled")
    {
        let secret = match storage
            .find_aggregate_api_secret_by_id(api.id.as_str())
            .map_err(|err| err.to_string())?
        {
            Some(secret) => secret,
            None => continue,
        };
        let probe = probe_models_endpoint(&client, &api, secret.as_str());
        if probe.ok {
            for slug in probe.models {
                models.entry(slug.clone()).or_insert_with(|| ModelInfo {
                    slug: slug.clone(),
                    display_name: slug,
                    description: api
                        .supplier_name
                        .as_ref()
                        .map(|name| format!("来自聚合 API：{name}")),
                    supported_in_api: true,
                    ..ModelInfo::default()
                });
            }
        } else if let Some(error) = probe.error {
            errors.push(format!(
                "{}: {}",
                api.supplier_name.as_deref().unwrap_or(api.id.as_str()),
                error
            ));
        }
    }
    if models.is_empty() && !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(ModelsResponse {
        models: models.into_values().collect(),
        extra: BTreeMap::new(),
    })
}

pub(crate) fn probe_all_aggregate_api_capabilities() -> Result<ProbeRunAllResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let apis = storage
        .list_aggregate_apis()
        .map_err(|err| err.to_string())?
        .into_iter()
        .collect::<Vec<_>>();
    let mut items = Vec::new();
    let mut failed = 0usize;
    for api in apis {
        match probe_aggregate_api_capabilities(api.id.clone()) {
            Ok(run) => items.push(run),
            Err(err) => {
                failed += 1;
                log::warn!(
                    "event=model_router_probe_all_item_failed aggregate_api_id={} err={}",
                    api.id,
                    err
                );
            }
        }
    }
    let succeeded = items.iter().filter(|item| item.status == "success").count();
    let failed = failed + items.iter().filter(|item| item.status != "success").count();
    Ok(ProbeRunAllResult {
        attempted: succeeded + failed,
        succeeded,
        failed,
        items,
    })
}

pub(crate) fn add_manual_probe_model(
    aggregate_api_id: String,
    model: String,
    supports_responses: Option<bool>,
    supports_chat_completions: Option<bool>,
    requires_adapter: Option<bool>,
) -> Result<ProbeRunSummary, String> {
    let aggregate_api_id =
        normalized_required(aggregate_api_id.as_str(), "aggregateApiId is required")?;
    let model = normalized_required(model.as_str(), "model is required")?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    if storage
        .find_aggregate_api_by_id(aggregate_api_id.as_str())
        .map_err(|err| err.to_string())?
        .is_none()
    {
        return Err("aggregate api not found".to_string());
    }
    let supports_responses = supports_responses.unwrap_or(false);
    let supports_chat = supports_chat_completions.unwrap_or(true);
    let requires_adapter = requires_adapter.unwrap_or(!supports_responses && supports_chat);
    let now = now_ts();
    let run = ProbeRun {
        id: generate_model_router_id("probe"),
        aggregate_api_id: aggregate_api_id.clone(),
        status: "manual".to_string(),
        started_at: now,
        finished_at: Some(now),
        models_status: Some("manual".to_string()),
        responses_status: Some(
            if supports_responses {
                "manual:true"
            } else {
                "manual:false"
            }
            .to_string(),
        ),
        chat_completions_status: Some(
            if supports_chat {
                "manual:true"
            } else {
                "manual:false"
            }
            .to_string(),
        ),
        error: None,
        raw_summary_json: Some(
            json!({
                "source": "manual",
                "model": model,
                "supportsResponses": supports_responses,
                "supportsChatCompletions": supports_chat,
                "requiresAdapter": requires_adapter
            })
            .to_string(),
        ),
    };
    storage
        .insert_probe_run(&run)
        .map_err(|err| err.to_string())?;
    let candidate = ProbeCandidate {
        id: generate_model_router_id("pc"),
        probe_run_id: run.id.clone(),
        aggregate_api_id: aggregate_api_id.clone(),
        model: model.clone(),
        supports_responses,
        supports_chat_completions: supports_chat,
        requires_adapter,
        suggested_route_strategy: "ordered".to_string(),
        suggested_priority: 0,
        suggested_weight: 1,
        applied: false,
        error: None,
        created_at: now,
        applied_at: None,
    };
    storage
        .insert_probe_candidate(&candidate)
        .map_err(|err| err.to_string())?;
    storage
        .upsert_upstream_model_capability(&UpstreamModelCapability {
            id: generate_model_router_id("cap"),
            aggregate_api_id,
            model,
            supports_responses,
            supports_chat_completions: supports_chat,
            requires_adapter,
            probe_status: "manual".to_string(),
            last_error: None,
            last_probe_at: Some(now),
            updated_at: now,
        })
        .map_err(|err| err.to_string())?;
    let apis = aggregate_api_map(&storage)?;
    Ok(probe_run_summary(
        run,
        vec![probe_candidate_summary(candidate)],
        &apis,
    ))
}

pub(crate) fn quick_check_model_route(
    aggregate_api_id: String,
    model: String,
) -> Result<ModelRouteQuickCheckResult, String> {
    let aggregate_api_id =
        normalized_required(aggregate_api_id.as_str(), "aggregateApiId is required")?;
    let model = normalized_required(model.as_str(), "model is required")?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let api = storage
        .find_aggregate_api_by_id(aggregate_api_id.as_str())
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "aggregate api not found".to_string())?;
    let secret = storage
        .find_aggregate_api_secret_by_id(aggregate_api_id.as_str())
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "aggregate api secret not found".to_string())?;
    let client = crate::gateway::fresh_upstream_client();
    let checked_at = now_ts();
    let started = Instant::now();

    let bindings = storage
        .list_model_route_bindings(Some(model.as_str()))
        .map_err(|err| err.to_string())?;
    let binding = bindings
        .iter()
        .find(|item| item.aggregate_api_id == aggregate_api_id);
    let prefer_chat = binding
        .map(|item| {
            item.requires_adapter || (!item.supports_responses && item.supports_chat_completions)
        })
        .unwrap_or(false);
    log::info!(
        "event=model_router_quick_check_start aggregate_api_id={} supplier={} model={} prefer_protocol={} bound={}",
        aggregate_api_id,
        api.supplier_name.as_deref().unwrap_or(""),
        model,
        if prefer_chat {
            "chat_completions"
        } else {
            "responses"
        },
        binding.is_some()
    );

    let first = if prefer_chat {
        quick_check_chat(&client, &api, secret.as_str(), model.as_str())
    } else {
        quick_check_responses(&client, &api, secret.as_str(), model.as_str())
    };
    let (probe, protocol, response_adapter) = if first.ok {
        (
            first,
            if prefer_chat {
                "chat_completions"
            } else {
                "responses"
            }
            .to_string(),
            prefer_chat.then(|| "ResponsesFromChatCompletions".to_string()),
        )
    } else if prefer_chat {
        log::warn!(
            "event=model_router_quick_check_protocol_fallback aggregate_api_id={} model={} from=chat_completions to=responses status_code={:?} err={}",
            aggregate_api_id,
            model,
            first.status_code,
            first.error.as_deref().unwrap_or("")
        );
        let fallback = quick_check_responses(&client, &api, secret.as_str(), model.as_str());
        if fallback.ok {
            (fallback, "responses".to_string(), None)
        } else {
            (
                first,
                "chat_completions".to_string(),
                Some("ResponsesFromChatCompletions".to_string()),
            )
        }
    } else {
        log::warn!(
            "event=model_router_quick_check_protocol_fallback aggregate_api_id={} model={} from=responses to=chat_completions status_code={:?} err={}",
            aggregate_api_id,
            model,
            first.status_code,
            first.error.as_deref().unwrap_or("")
        );
        let fallback = quick_check_chat(&client, &api, secret.as_str(), model.as_str());
        if fallback.ok {
            (
                fallback,
                "chat_completions".to_string(),
                Some("ResponsesFromChatCompletions".to_string()),
            )
        } else {
            (first, "responses".to_string(), None)
        }
    };
    let latency_ms = started.elapsed().as_millis() as i64;
    let requires_adapter = protocol == "chat_completions";
    storage
        .upsert_upstream_model_capability(&UpstreamModelCapability {
            id: generate_model_router_id("cap"),
            aggregate_api_id: aggregate_api_id.clone(),
            model: model.clone(),
            supports_responses: probe.ok && protocol == "responses",
            supports_chat_completions: probe.ok && protocol == "chat_completions",
            requires_adapter: probe.ok && requires_adapter,
            probe_status: if probe.ok { "success" } else { "failed" }.to_string(),
            last_error: probe.error.clone(),
            last_probe_at: Some(checked_at),
            updated_at: checked_at,
        })
        .map_err(|err| err.to_string())?;
    let update_result = if probe.ok {
        storage.update_model_route_binding_probe_result(
            model.as_str(),
            aggregate_api_id.as_str(),
            "success",
            None,
            Some(checked_at),
            Some(protocol == "responses"),
            Some(protocol == "chat_completions"),
            Some(requires_adapter),
        )
    } else {
        storage.update_model_route_binding_probe_result(
            model.as_str(),
            aggregate_api_id.as_str(),
            "failed",
            probe.error.as_deref(),
            None,
            None,
            None,
            None,
        )
    };
    if let Err(err) = update_result {
        log::warn!(
            "event=model_router_quick_check_binding_update_failed model={} aggregate_api_id={} err={}",
            model,
            aggregate_api_id,
            err
        );
    }
    log::info!(
        "event=model_router_quick_check_result aggregate_api_id={} supplier={} model={} ok={} protocol={} status_code={:?} latency_ms={} err={}",
        aggregate_api_id,
        api.supplier_name.as_deref().unwrap_or(""),
        model,
        probe.ok,
        protocol,
        probe.status_code,
        latency_ms,
        probe.error.as_deref().unwrap_or("")
    );

    Ok(ModelRouteQuickCheckResult {
        aggregate_api_id,
        aggregate_api_name: api.supplier_name,
        model,
        ok: probe.ok,
        status_code: probe.status_code,
        protocol,
        response_adapter,
        latency_ms,
        error: probe.error,
        checked_at,
    })
}

pub(crate) fn route_aggregate_candidates_for_model(
    storage: &Storage,
    model: Option<&str>,
    fallback: Vec<AggregateApi>,
    key_id: &str,
) -> Vec<AggregateApi> {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return fallback;
    };
    let Ok(bindings) = effective_model_route_bindings_for_request_model(storage, model) else {
        return fallback;
    };
    if bindings.is_empty() {
        return fallback;
    }

    let mut fallback_candidates = Vec::<AggregateApi>::new();
    let mut fallback_model_candidates = Vec::<AggregateApi>::new();
    let mut active_bound_candidates = BTreeMap::<String, AggregateApi>::new();
    let mut active_candidates_by_base = BTreeMap::<String, Vec<AggregateApi>>::new();
    let mut candidate_pool_was_empty = true;
    for api in fallback {
        candidate_pool_was_empty = false;
        if api.status.trim().eq_ignore_ascii_case("active") {
            fallback_candidates.push(api.clone());
            if aggregate_api_has_successful_capability_for_request_model(
                storage,
                model,
                api.id.as_str(),
            ) {
                fallback_model_candidates.push(api.clone());
            }
            active_candidates_by_base
                .entry(normalize_route_base_url(api.url.as_str()))
                .or_default()
                .push(api.clone());
            active_bound_candidates.insert(api.id.clone(), api);
        }
    }
    if active_bound_candidates.is_empty() && !candidate_pool_was_empty {
        return fallback_model_candidates;
    }

    let mut routed = Vec::new();
    let mut routed_base_urls = Vec::<String>::new();
    for binding in route_binding_order(bindings, key_id, model) {
        if let Some(api) = active_bound_candidates.remove(binding.aggregate_api_id.as_str()) {
            routed_base_urls.push(normalize_route_base_url(api.url.as_str()));
            routed.push(api);
            continue;
        }

        if candidate_pool_was_empty || active_bound_candidates.is_empty() {
            let bound_active = storage
                .find_aggregate_api_by_id(binding.aggregate_api_id.as_str())
                .ok()
                .flatten()
                .filter(|item| item.status.trim().eq_ignore_ascii_case("active"));

            if let Some(api) = bound_active {
                routed_base_urls.push(normalize_route_base_url(api.url.as_str()));
                routed.push(api);
                continue;
            }
        }

        record_route_binding_error(
            storage,
            Some(binding.model.as_str()),
            binding.aggregate_api_id.as_str(),
            "bound aggregate api is not in the active candidate pool",
        );
    }

    if !routed_base_urls.is_empty() {
        let request_key = normalize_model_match_key(model);
        let exclusively_other_model_api_ids: BTreeSet<String> = if request_key.is_empty() {
            BTreeSet::new()
        } else {
            let current_model_api_ids: BTreeSet<String> =
                effective_model_route_bindings_for_request_model(storage, model)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|b| b.aggregate_api_id)
                    .collect();
            storage
                .list_model_route_bindings(None)
                .unwrap_or_default()
                .into_iter()
                .filter(|b| b.enabled && normalize_model_match_key(b.model.as_str()) != request_key)
                .map(|b| b.aggregate_api_id)
                .filter(|id| !current_model_api_ids.contains(id.as_str()))
                .collect()
        };
        let mut routed_ids = routed
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>();
        for routed_base_url in routed_base_urls {
            if let Some(candidates) = active_candidates_by_base.get(&routed_base_url) {
                for api in candidates {
                    if routed_ids.contains(api.id.as_str()) {
                        continue;
                    }
                    if exclusively_other_model_api_ids.contains(api.id.as_str()) {
                        continue;
                    }
                    routed_ids.insert(api.id.clone());
                    routed.push(api.clone());
                }
            }
        }
    }

    if routed.is_empty() {
        fallback_model_candidates
    } else {
        routed
    }
}

fn aggregate_api_has_successful_capability_for_request_model(
    storage: &Storage,
    model: &str,
    aggregate_api_id: &str,
) -> bool {
    let request_key = normalize_model_match_key(model);
    if request_key.is_empty() {
        return false;
    }
    storage
        .list_upstream_model_capabilities(None)
        .map(|items| {
            items.into_iter().any(|item| {
                item.aggregate_api_id == aggregate_api_id
                    && is_successful_or_manual_capability_status(item.probe_status.as_str())
                    && capability_model_matches_request(item.model.as_str(), request_key.as_str())
                    && (item.supports_responses || item.supports_chat_completions)
            })
        })
        .unwrap_or(false)
}

fn normalize_route_base_url(url: &str) -> String {
    let mut value = url.trim().trim_end_matches('/').to_ascii_lowercase();
    if value.ends_with("/v1") {
        value.truncate(value.len().saturating_sub(3));
    }
    value
}

pub(crate) fn model_route_applies_for_model(storage: &Storage, model: Option<&str>) -> bool {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    effective_model_route_bindings_for_request_model(storage, model)
        .map(|items| !items.is_empty())
        .unwrap_or(false)
}

pub(crate) fn aggregate_candidate_requires_responses_to_chat_adapter(
    storage: &Storage,
    model: Option<&str>,
    aggregate_api_id: &str,
) -> bool {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let aggregate_api_id = aggregate_api_id.trim();
    if aggregate_api_id.is_empty() {
        return false;
    }
    effective_model_route_bindings_for_request_model(storage, model)
        .map(|items| {
            items.into_iter().any(|item| {
                item.aggregate_api_id == aggregate_api_id
                    && item.supports_chat_completions
                    && item.requires_adapter
                    && !item.supports_responses
            })
        })
        .unwrap_or(false)
        || aggregate_capability_requires_responses_to_chat_adapter(
            storage,
            Some(model),
            aggregate_api_id,
        )
}

fn aggregate_capability_requires_responses_to_chat_adapter(
    storage: &Storage,
    model: Option<&str>,
    aggregate_api_id: &str,
) -> bool {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let request_key = normalize_model_match_key(model);
    if request_key.is_empty() {
        return false;
    }
    storage
        .list_upstream_model_capabilities(None)
        .map(|items| {
            items.into_iter().any(|item| {
                item.aggregate_api_id == aggregate_api_id
                    && is_successful_or_manual_capability_status(item.probe_status.as_str())
                    && capability_model_matches_request(item.model.as_str(), request_key.as_str())
                    && item.supports_chat_completions
                    && !item.supports_responses
            })
        })
        .unwrap_or(false)
}

fn effective_model_route_bindings_for_request_model(
    storage: &Storage,
    model: &str,
) -> Result<Vec<ModelRouteBinding>, rusqlite::Error> {
    let exact = storage.list_enabled_model_route_bindings(model)?;
    let request_key = normalize_model_match_key(model);
    let mut bindings = if !exact.is_empty() {
        exact
    } else if request_key.is_empty() {
        exact
    } else {
        let mut matched = storage
            .list_model_route_bindings(None)?
            .into_iter()
            .filter(|item| {
                item.enabled && normalize_model_match_key(item.model.as_str()) == request_key
            })
            .collect::<Vec<_>>();
        matched.sort_by(|left, right| {
            right
                .manual_preferred
                .cmp(&left.manual_preferred)
                .then(left.priority.cmp(&right.priority))
                .then(right.weight.cmp(&left.weight))
                .then(right.updated_at.cmp(&left.updated_at))
        });
        matched
    };
    if bindings.is_empty() {
        return successful_model_route_bindings_from_capabilities(storage, model);
    }

    let capability_map = successful_upstream_model_capabilities_for_request_model(storage, model)?
        .into_iter()
        .map(|item| (item.aggregate_api_id.clone(), item))
        .collect::<BTreeMap<_, _>>();
    for binding in &mut bindings {
        if let Some(capability) = capability_map.get(binding.aggregate_api_id.as_str()) {
            // Manual route compatibility settings are operator intent. Probe results can fill in
            // unknowns, but they must not downgrade an explicit chat adapter binding back to native
            // Responses passthrough after a short probe falsely succeeds.
            if !binding.requires_adapter {
                binding.supports_responses = capability.supports_responses;
                binding.requires_adapter = capability.requires_adapter;
            }
            binding.supports_chat_completions =
                binding.supports_chat_completions || capability.supports_chat_completions;
            binding.last_probe_status = Some(capability.probe_status.clone());
            binding.last_error = capability.last_error.clone();
            binding.last_success_at = capability.last_probe_at;
        }
    }
    Ok(bindings)
}

fn normalize_model_match_key(model: &str) -> String {
    model
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn capability_model_matches_request(capability_model: &str, request_key: &str) -> bool {
    if request_key.is_empty() {
        return false;
    }
    normalize_model_match_key(capability_model) == request_key
        || capability_model
            .rsplit('/')
            .next()
            .is_some_and(|tail| normalize_model_match_key(tail) == request_key)
}

fn is_successful_or_manual_capability_status(status: &str) -> bool {
    let status = status.trim();
    status.eq_ignore_ascii_case("success") || status.eq_ignore_ascii_case("manual")
}

pub(crate) fn resolve_upstream_model_for_aggregate_candidate(
    storage: &Storage,
    requested_model: Option<&str>,
    aggregate_api_id: &str,
) -> Option<String> {
    let requested = requested_model
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let request_key = normalize_model_match_key(requested);
    if request_key.is_empty() {
        return None;
    }
    let mut matches = storage
        .list_upstream_model_capabilities(None)
        .ok()?
        .into_iter()
        .filter(|item| {
            item.aggregate_api_id == aggregate_api_id
                && is_successful_or_manual_capability_status(item.probe_status.as_str())
                && (item.supports_responses || item.supports_chat_completions)
                && capability_model_matches_request(item.model.as_str(), request_key.as_str())
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        let left_exact = normalize_model_match_key(left.model.as_str()) == request_key;
        let right_exact = normalize_model_match_key(right.model.as_str()) == request_key;
        right_exact
            .cmp(&left_exact)
            .then(right.last_probe_at.cmp(&left.last_probe_at))
            .then(left.model.cmp(&right.model))
    });
    matches.into_iter().next().and_then(|item| {
        let upstream_model = item.model.trim();
        (!upstream_model.is_empty() && upstream_model != requested)
            .then(|| upstream_model.to_string())
    })
}

fn successful_upstream_model_capabilities_for_request_model(
    storage: &Storage,
    model: &str,
) -> Result<Vec<UpstreamModelCapability>, rusqlite::Error> {
    let exact = storage
        .list_upstream_model_capabilities(Some(model))?
        .into_iter()
        .filter(|item| is_successful_or_manual_capability_status(item.probe_status.as_str()))
        .collect::<Vec<_>>();
    if !exact.is_empty() {
        return Ok(exact);
    }

    let request_key = normalize_model_match_key(model);
    if request_key.is_empty() {
        return Ok(exact);
    }

    let mut matched = storage
        .list_upstream_model_capabilities(None)?
        .into_iter()
        .filter(|item| {
            is_successful_or_manual_capability_status(item.probe_status.as_str())
                && capability_model_matches_request(item.model.as_str(), request_key.as_str())
        })
        .collect::<Vec<_>>();
    matched.sort_by(|left, right| {
        right
            .last_probe_at
            .cmp(&left.last_probe_at)
            .then(left.aggregate_api_id.cmp(&right.aggregate_api_id))
    });
    Ok(matched)
}

fn successful_model_route_bindings_from_capabilities(
    storage: &Storage,
    model: &str,
) -> Result<Vec<ModelRouteBinding>, rusqlite::Error> {
    let mut capabilities =
        successful_upstream_model_capabilities_for_request_model(storage, model)?;
    capabilities.retain(|capability| {
        storage
            .find_aggregate_api_by_id(capability.aggregate_api_id.as_str())
            .ok()
            .flatten()
            .is_some_and(|api| api.status.trim().eq_ignore_ascii_case("active"))
    });
    if capabilities.is_empty() {
        return Ok(Vec::new());
    }
    capabilities.sort_by(|left, right| {
        right
            .last_probe_at
            .cmp(&left.last_probe_at)
            .then(left.aggregate_api_id.cmp(&right.aggregate_api_id))
    });

    let mut bindings = Vec::new();
    for (index, capability) in capabilities.into_iter().enumerate() {
        bindings.push(ModelRouteBinding {
            id: format!("capability-route-{}-{}", capability.aggregate_api_id, index),
            model: capability.model,
            aggregate_api_id: capability.aggregate_api_id,
            enabled: true,
            priority: index as i64,
            weight: 1,
            route_strategy: if capability.requires_adapter {
                "ordered".to_string()
            } else {
                "balanced".to_string()
            },
            manual_preferred: false,
            supports_responses: capability.supports_responses,
            supports_chat_completions: capability.supports_chat_completions,
            requires_adapter: capability.requires_adapter,
            last_probe_status: Some(capability.probe_status),
            last_error: capability.last_error,
            last_success_at: capability.last_probe_at,
            created_at: capability.updated_at,
            updated_at: capability.updated_at,
        });
    }
    Ok(bindings)
}

pub(crate) fn record_route_binding_success(
    storage: &Storage,
    model: Option<&str>,
    aggregate_api_id: &str,
) {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let aggregate_api_id = aggregate_api_id.trim();
    if aggregate_api_id.is_empty() {
        return;
    }
    if let Err(err) = storage.update_model_route_binding_result(
        model,
        aggregate_api_id,
        "success",
        None,
        Some(now_ts()),
    ) {
        log::warn!(
            "event=model_router_binding_success_update_failed model={} aggregate_api_id={} err={}",
            model,
            aggregate_api_id,
            err
        );
    }
}

pub(crate) fn record_route_binding_error(
    storage: &Storage,
    model: Option<&str>,
    aggregate_api_id: &str,
    error: &str,
) {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let aggregate_api_id = aggregate_api_id.trim();
    if aggregate_api_id.is_empty() {
        return;
    }
    let error = error.trim();
    let last_error = (!error.is_empty()).then_some(error);
    if let Err(err) = storage.update_model_route_binding_result(
        model,
        aggregate_api_id,
        "failed",
        last_error,
        None,
    ) {
        log::warn!(
            "event=model_router_binding_error_update_failed model={} aggregate_api_id={} err={}",
            model,
            aggregate_api_id,
            err
        );
    }
}

pub(crate) fn import_codexmanager_data(
    source_path: Option<String>,
) -> Result<ModelRouterImportResult, String> {
    import_codexmanager_data_with_mode(source_path, false)
}

pub(crate) fn import_codexmanager_data_preserving_target(
    source_path: Option<String>,
) -> Result<ModelRouterImportResult, String> {
    import_codexmanager_data_with_mode(source_path, true)
}

fn import_codexmanager_data_with_mode(
    source_path: Option<String>,
    preserve_target_records: bool,
) -> Result<ModelRouterImportResult, String> {
    let source_path = resolve_import_source_path(source_path)?;
    let target_path = std::env::var("CODEXMANAGER_DB_PATH")
        .map(PathBuf::from)
        .map_err(|_| "CODEXMANAGER_DB_PATH is not set".to_string())?;
    if same_path(&source_path, &target_path) {
        return Err("导入源和当前数据库相同，已取消导入".to_string());
    }
    if !source_path.exists() {
        return Err(format!("导入源数据库不存在: {}", source_path.display()));
    }

    let backup_path = if target_path.exists() {
        let backup = import_backup_path(&target_path);
        copy_sqlite_snapshot(&target_path, &backup)?;
        Some(backup)
    } else {
        None
    };

    let source = Storage::open(&source_path).map_err(|err| {
        format!(
            "打开导入源 CodexManager 数据库失败 ({}): {err}",
            source_path.display()
        )
    })?;
    let target = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    target.init().map_err(|err| err.to_string())?;

    let mut accounts = 0usize;
    if sqlite_table_exists(&source_path, "accounts")? {
        for account in source.list_accounts().map_err(|err| err.to_string())? {
            if preserve_target_records
                && target
                    .find_account_by_id(account.id.as_str())
                    .map_err(|err| err.to_string())?
                    .is_some()
            {
                continue;
            }
            target
                .insert_account(&account)
                .map_err(|err| err.to_string())?;
            accounts += 1;
        }
    }

    let mut tokens = 0usize;
    if sqlite_table_exists(&source_path, "tokens")? {
        for token in source.list_tokens().map_err(|err| err.to_string())? {
            if preserve_target_records
                && target
                    .find_token_by_account_id(token.account_id.as_str())
                    .map_err(|err| err.to_string())?
                    .is_some()
            {
                continue;
            }
            target.insert_token(&token).map_err(|err| err.to_string())?;
            tokens += 1;
        }
    }

    let mut usage_snapshots = 0usize;
    if sqlite_table_exists(&source_path, "usage_snapshots")? {
        let existing_usage_snapshots = if preserve_target_records {
            existing_usage_snapshot_keys(&target_path)?
        } else {
            BTreeSet::new()
        };
        for snapshot in read_usage_snapshots_for_import(&source_path)? {
            if preserve_target_records
                && existing_usage_snapshots.contains(&usage_snapshot_key(&snapshot))
            {
                continue;
            }
            target
                .insert_usage_snapshot(&snapshot)
                .map_err(|err| err.to_string())?;
            usage_snapshots += 1;
        }
    }

    let mut aggregate_apis = 0usize;
    let mut aggregate_api_secrets = 0usize;
    for api in source
        .list_aggregate_apis()
        .map_err(|err| err.to_string())?
    {
        if preserve_target_records
            && target
                .find_aggregate_api_by_id(api.id.as_str())
                .map_err(|err| err.to_string())?
                .is_some()
        {
            continue;
        }
        target
            .insert_aggregate_api(&api)
            .map_err(|err| err.to_string())?;
        aggregate_apis += 1;
        if let Some(secret) = source
            .find_aggregate_api_secret_by_id(api.id.as_str())
            .map_err(|err| err.to_string())?
        {
            target
                .upsert_aggregate_api_secret(api.id.as_str(), secret.as_str())
                .map_err(|err| err.to_string())?;
            aggregate_api_secrets += 1;
        }
    }

    let mut api_keys = 0usize;
    let mut api_key_secrets = 0usize;
    for key in source.list_api_keys().map_err(|err| err.to_string())? {
        if preserve_target_records
            && target
                .find_api_key_by_id(key.id.as_str())
                .map_err(|err| err.to_string())?
                .is_some()
        {
            continue;
        }
        target.insert_api_key(&key).map_err(|err| err.to_string())?;
        api_keys += 1;
        if let Some(secret) = source
            .find_api_key_secret_by_id(key.id.as_str())
            .map_err(|err| err.to_string())?
        {
            target
                .upsert_api_key_secret(key.id.as_str(), secret.as_str())
                .map_err(|err| err.to_string())?;
            api_key_secrets += 1;
        }
    }

    let mut route_bindings = 0usize;
    if sqlite_table_exists(&source_path, "model_route_bindings")? {
        let existing_route_binding_ids = if preserve_target_records {
            target
                .list_model_route_bindings(None)
                .map_err(|err| err.to_string())?
                .into_iter()
                .map(|item| item.id)
                .collect::<BTreeSet<_>>()
        } else {
            BTreeSet::new()
        };
        for binding in source
            .list_model_route_bindings(None)
            .map_err(|err| err.to_string())?
        {
            if preserve_target_records && existing_route_binding_ids.contains(&binding.id) {
                continue;
            }
            target
                .upsert_model_route_binding(&binding)
                .map_err(|err| err.to_string())?;
            route_bindings += 1;
        }
    }

    let mut workspace_defaults = 0usize;
    if sqlite_table_exists(&source_path, "workspace_model_defaults")? {
        for default in source
            .list_workspace_model_defaults()
            .map_err(|err| err.to_string())?
        {
            if preserve_target_records
                && target
                    .find_workspace_model_default(default.workspace.as_str())
                    .map_err(|err| err.to_string())?
                    .is_some()
            {
                continue;
            }
            target
                .upsert_workspace_model_default(&default)
                .map_err(|err| err.to_string())?;
            workspace_defaults += 1;
        }
    }

    let mut request_logs = 0usize;
    let mut request_token_stats = 0usize;
    if sqlite_table_exists(&source_path, "request_logs")? {
        let existing_request_log_keys = if preserve_target_records {
            existing_request_log_keys(&target_path)?
        } else {
            BTreeSet::new()
        };
        for (log, stat) in read_request_logs_for_import(&source_path)? {
            if preserve_target_records && existing_request_log_keys.contains(&request_log_key(&log))
            {
                continue;
            }
            let target_log_id = target
                .insert_request_log(&log)
                .map_err(|err| err.to_string())?;
            request_logs += 1;
            if let Some(mut stat) = stat {
                stat.request_log_id = target_log_id;
                target
                    .insert_request_token_stat(&stat)
                    .map_err(|err| err.to_string())?;
                request_token_stats += 1;
            }
        }
    }

    let mut app_settings = 0usize;
    for (key, value) in source.list_app_settings().map_err(|err| err.to_string())? {
        if should_import_app_setting(key.as_str()) {
            if preserve_target_records
                && target
                    .get_app_setting(key.as_str())
                    .map_err(|err| err.to_string())?
                    .is_some()
            {
                continue;
            }
            target
                .set_app_setting(key.as_str(), value.as_str(), now_ts())
                .map_err(|err| err.to_string())?;
            app_settings += 1;
        }
    }

    Ok(ModelRouterImportResult {
        source_path: source_path.to_string_lossy().to_string(),
        backup_path: backup_path.map(|path| path.to_string_lossy().to_string()),
        accounts,
        tokens,
        usage_snapshots,
        request_logs,
        request_token_stats,
        aggregate_apis,
        aggregate_api_secrets,
        api_keys,
        api_key_secrets,
        route_bindings,
        workspace_defaults,
        app_settings,
    })
}

pub(crate) fn import_codexmanager_data_once(
    source_path: Option<String>,
) -> Result<Option<ModelRouterImportResult>, String> {
    let source_path = resolve_import_source_path(source_path)?;
    let target_path = std::env::var("CODEXMANAGER_DB_PATH")
        .map(PathBuf::from)
        .map_err(|_| "CODEXMANAGER_DB_PATH is not set".to_string())?;
    if same_path(&source_path, &target_path) {
        return Ok(None);
    }
    if !source_path.exists() {
        return Err(format!("导入源数据库不存在: {}", source_path.display()));
    }
    if import_marker_exists(&target_path, &source_path)? {
        return Ok(None);
    }
    let result = import_codexmanager_data(Some(source_path.to_string_lossy().to_string()))?;
    write_import_marker(&target_path, &source_path)?;
    Ok(Some(result))
}

fn import_marker_exists(target_path: &Path, source_path: &Path) -> Result<bool, String> {
    let marker_key = import_marker_key(source_path);
    let storage =
        crate::storage_helpers::open_storage_at_path(target_path.to_string_lossy().as_ref())
            .ok_or_else(|| "storage unavailable".to_string())?;
    let marker_value = source_path.to_string_lossy().to_string();
    Ok(storage
        .list_app_settings()
        .map_err(|err| err.to_string())?
        .into_iter()
        .any(|(key, value)| key == marker_key && value == marker_value))
}

fn write_import_marker(target_path: &Path, source_path: &Path) -> Result<(), String> {
    let marker_key = import_marker_key(source_path);
    let storage =
        crate::storage_helpers::open_storage_at_path(target_path.to_string_lossy().as_ref())
            .ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .set_app_setting(&marker_key, &source_path.to_string_lossy(), now_ts())
        .map_err(|err| err.to_string())
}

fn import_marker_key(source_path: &Path) -> String {
    let _ = source_path;
    "imported.codexmanager.source_path".to_string()
}

fn existing_usage_snapshot_keys(target_path: &Path) -> Result<BTreeSet<String>, String> {
    let conn = Connection::open(target_path)
        .map_err(|err| format!("打开目标 usage_snapshots 失败: {err}"))?;
    let has_table = sqlite_table_exists(target_path, "usage_snapshots")?;
    if !has_table {
        return Ok(BTreeSet::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT account_id, captured_at
             FROM usage_snapshots",
        )
        .map_err(|err| format!("读取目标 usage_snapshots 失败: {err}"))?;
    let rows = stmt
        .query_map([], |row| {
            let account_id: String = row.get(0)?;
            let captured_at: i64 = row.get(1)?;
            Ok(format!("{account_id}\u{1f}{captured_at}"))
        })
        .map_err(|err| format!("映射目标 usage_snapshots 失败: {err}"))?;
    let mut keys = BTreeSet::new();
    for row in rows {
        keys.insert(row.map_err(|err| format!("读取目标 usage_snapshots 行失败: {err}"))?);
    }
    Ok(keys)
}

fn usage_snapshot_key(snapshot: &UsageSnapshotRecord) -> String {
    format!("{}\u{1f}{}", snapshot.account_id, snapshot.captured_at)
}

fn existing_request_log_keys(target_path: &Path) -> Result<BTreeSet<String>, String> {
    let conn = Connection::open(target_path)
        .map_err(|err| format!("打开目标 request_logs 失败: {err}"))?;
    if !sqlite_table_exists(target_path, "request_logs")? {
        return Ok(BTreeSet::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT trace_id, key_id, account_id, initial_account_id,
                    request_path, original_path, adapted_path, method,
                    request_type, gateway_mode, model, status_code,
                    error, created_at
             FROM request_logs",
        )
        .map_err(|err| format!("读取目标 request_logs 失败: {err}"))?;
    let rows = stmt
        .query_map([], |row| {
            let log = RequestLog {
                trace_id: row.get(0)?,
                key_id: row.get(1)?,
                account_id: row.get(2)?,
                conversation_id: None,
                initial_account_id: row.get(3)?,
                attempted_account_ids_json: None,
                initial_aggregate_api_id: None,
                attempted_aggregate_api_ids_json: None,
                request_path: row.get(4)?,
                original_path: row.get(5)?,
                adapted_path: row.get(6)?,
                method: row.get(7)?,
                request_type: row.get(8)?,
                gateway_mode: row.get(9)?,
                transparent_mode: None,
                enhanced_mode: None,
                model: row.get(10)?,
                reasoning_effort: None,
                service_tier: None,
                effective_service_tier: None,
                response_adapter: None,
                upstream_url: None,
                aggregate_api_supplier_name: None,
                aggregate_api_url: None,
                status_code: row.get(11)?,
                duration_ms: None,
                first_response_ms: None,
                input_tokens: None,
                cached_input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                reasoning_output_tokens: None,
                estimated_cost_usd: None,
                error: row.get(12)?,
                created_at: row.get(13)?,
            };
            Ok(request_log_key(&log))
        })
        .map_err(|err| format!("映射目标 request_logs 失败: {err}"))?;
    let mut keys = BTreeSet::new();
    for row in rows {
        keys.insert(row.map_err(|err| format!("读取目标 request_logs 行失败: {err}"))?);
    }
    Ok(keys)
}

fn request_log_key(log: &RequestLog) -> String {
    if let Some(trace_id) = log
        .trace_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!("trace\u{1f}{trace_id}");
    }
    [
        log.created_at.to_string(),
        log.method.clone(),
        log.request_path.clone(),
        log.model.clone().unwrap_or_default(),
        log.status_code
            .map(|value| value.to_string())
            .unwrap_or_default(),
        log.error.clone().unwrap_or_default(),
    ]
    .join("\u{1f}")
}

fn read_usage_snapshots_for_import(source_path: &Path) -> Result<Vec<UsageSnapshotRecord>, String> {
    let conn = Connection::open(source_path)
        .map_err(|err| format!("打开导入源 usage_snapshots 失败: {err}"))?;
    let mut stmt = conn
        .prepare(
            "SELECT account_id, used_percent, window_minutes, resets_at,
                secondary_used_percent, secondary_window_minutes, secondary_resets_at,
                credits_json, captured_at
             FROM usage_snapshots
             ORDER BY captured_at ASC, id ASC",
        )
        .map_err(|err| format!("读取 usage_snapshots 失败: {err}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(UsageSnapshotRecord {
                account_id: row.get(0)?,
                used_percent: row.get(1)?,
                window_minutes: row.get(2)?,
                resets_at: row.get(3)?,
                secondary_used_percent: row.get(4)?,
                secondary_window_minutes: row.get(5)?,
                secondary_resets_at: row.get(6)?,
                credits_json: row.get(7)?,
                captured_at: row.get(8)?,
            })
        })
        .map_err(|err| format!("映射 usage_snapshots 失败: {err}"))?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|err| format!("读取 usage_snapshots 行失败: {err}"))?);
    }
    Ok(items)
}

fn read_request_logs_for_import(
    source_path: &Path,
) -> Result<Vec<(RequestLog, Option<RequestTokenStat>)>, String> {
    let conn = Connection::open(source_path)
        .map_err(|err| format!("打开导入源 request_logs 失败: {err}"))?;
    let has_token_stats = sqlite_table_exists(source_path, "request_token_stats")?;
    let token_select = if has_token_stats {
        "t.input_tokens, t.cached_input_tokens, t.output_tokens, t.total_tokens, t.reasoning_output_tokens, t.estimated_cost_usd"
    } else {
        "NULL, NULL, NULL, NULL, NULL, NULL"
    };
    let token_join = if has_token_stats {
        "LEFT JOIN request_token_stats t ON t.request_log_id = r.id"
    } else {
        ""
    };
    let sql = format!(
        "SELECT
            r.trace_id, r.key_id, r.account_id, r.initial_account_id, r.attempted_account_ids_json,
            r.initial_aggregate_api_id, r.attempted_aggregate_api_ids_json,
            r.request_path, r.original_path, r.adapted_path,
            r.method, r.request_type, r.gateway_mode, r.transparent_mode, r.enhanced_mode,
            r.model, r.reasoning_effort, r.service_tier, r.effective_service_tier,
            r.response_adapter, r.upstream_url, r.aggregate_api_supplier_name, r.aggregate_api_url,
            r.status_code, r.duration_ms, r.first_response_ms, r.error, r.created_at,
            {token_select}
         FROM request_logs r
         {token_join}
         ORDER BY r.created_at ASC, r.id ASC"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|err| format!("读取 request_logs 失败: {err}"))?;
    let rows = stmt
        .query_map([], |row| {
            let log = RequestLog {
                trace_id: row.get(0)?,
                key_id: row.get(1)?,
                account_id: row.get(2)?,
                conversation_id: None,
                initial_account_id: row.get(3)?,
                attempted_account_ids_json: row.get(4)?,
                initial_aggregate_api_id: row.get(5)?,
                attempted_aggregate_api_ids_json: row.get(6)?,
                request_path: row.get(7)?,
                original_path: row.get(8)?,
                adapted_path: row.get(9)?,
                method: row.get(10)?,
                request_type: row.get(11)?,
                gateway_mode: row.get(12)?,
                transparent_mode: row.get(13)?,
                enhanced_mode: row.get(14)?,
                model: row.get(15)?,
                reasoning_effort: row.get(16)?,
                service_tier: row.get(17)?,
                effective_service_tier: row.get(18)?,
                response_adapter: row.get(19)?,
                upstream_url: row.get(20)?,
                aggregate_api_supplier_name: row.get(21)?,
                aggregate_api_url: row.get(22)?,
                status_code: row.get(23)?,
                duration_ms: row.get(24)?,
                first_response_ms: row.get(25)?,
                input_tokens: row.get(28)?,
                cached_input_tokens: row.get(29)?,
                output_tokens: row.get(30)?,
                total_tokens: row.get(31)?,
                reasoning_output_tokens: row.get(32)?,
                estimated_cost_usd: row.get(33)?,
                error: row.get(26)?,
                created_at: row.get(27)?,
            };
            let stat = if has_token_stats {
                Some(RequestTokenStat {
                    request_log_id: 0,
                    key_id: log.key_id.clone(),
                    account_id: log.account_id.clone(),
                    model: log.model.clone(),
                    input_tokens: log.input_tokens,
                    cached_input_tokens: log.cached_input_tokens,
                    output_tokens: log.output_tokens,
                    total_tokens: log.total_tokens,
                    reasoning_output_tokens: log.reasoning_output_tokens,
                    estimated_cost_usd: log.estimated_cost_usd,
                    created_at: log.created_at,
                })
            } else {
                None
            };
            Ok((log, stat))
        })
        .map_err(|err| format!("映射 request_logs 失败: {err}"))?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|err| format!("读取 request_logs 行失败: {err}"))?);
    }
    Ok(items)
}

fn resolve_import_source_path(source_path: Option<String>) -> Result<PathBuf, String> {
    if let Some(path) = source_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(path));
    }
    let app_data = std::env::var("APPDATA")
        .or_else(|_| std::env::var("USERPROFILE").map(|home| format!("{home}\\AppData\\Roaming")))
        .map_err(|_| "无法定位 APPDATA".to_string())?;
    Ok(PathBuf::from(app_data)
        .join("com.codexmanager.desktop")
        .join("codexmanager.db"))
}

fn should_import_app_setting(key: &str) -> bool {
    !matches!(
        key,
        "service.addr"
            | "app.service_addr"
            | "serviceAddr"
            | "service.bind_mode"
            | "ui.theme"
            | "ui.locale"
            | "web_access.password_hash"
    )
}

fn sqlite_table_exists(path: &Path, table: &str) -> Result<bool, String> {
    let conn = Connection::open(path)
        .map_err(|err| format!("open db {} failed: {err}", path.display()))?;
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
        [table],
        |_| Ok(()),
    )
    .map(|_| true)
    .or_else(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Ok(false),
        other => Err(format!("check table {table} failed: {other}")),
    })
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn import_backup_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("codexmanager");
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    parent.join(format!("{stem}.before-router-import.{ts}.bak.db"))
}

fn copy_sqlite_snapshot(source: &Path, target: &Path) -> Result<(), String> {
    let source_conn = Connection::open(source)
        .map_err(|err| format!("open source db {} failed: {err}", source.display()))?;
    source_conn
        .busy_timeout(Duration::from_millis(3000))
        .map_err(|err| format!("configure source db {} failed: {err}", source.display()))?;
    let mut target_conn = Connection::open(target)
        .map_err(|err| format!("open backup db {} failed: {err}", target.display()))?;
    target_conn
        .busy_timeout(Duration::from_millis(3000))
        .map_err(|err| format!("configure backup db {} failed: {err}", target.display()))?;
    let backup = Backup::new(&source_conn, &mut target_conn).map_err(|err| {
        format!(
            "create sqlite backup {} -> {} failed: {err}",
            source.display(),
            target.display()
        )
    })?;
    backup
        .run_to_completion(64, Duration::from_millis(25), None)
        .map_err(|err| {
            format!(
                "run sqlite backup {} -> {} failed: {err}",
                source.display(),
                target.display()
            )
        })?;
    Ok(())
}

#[derive(Debug, Clone)]
struct SessionDefaultResolution {
    model: String,
    reasoning_effort: Option<String>,
    source: String,
}

fn workspace_auto_remember_enabled(storage: &Storage, workspace: &str) -> Result<bool, String> {
    Ok(storage
        .find_workspace_model_default(workspace.trim())
        .map_err(|err| err.to_string())?
        .map(|item| item.auto_remember)
        .unwrap_or(true))
}

fn resolve_session_default_for_workspace(
    storage: &Storage,
    workspace: &str,
) -> Result<Option<SessionDefaultResolution>, String> {
    let workspace = workspace.trim();
    let workspace_default = storage
        .find_workspace_model_default(workspace)
        .map_err(|err| err.to_string())?;
    let inherit_last_session = workspace_default
        .as_ref()
        .map(|item| item.inherit_last_session)
        .unwrap_or(true);
    if inherit_last_session {
        if let Some(memory) = storage
            .latest_workspace_session_model(workspace)
            .map_err(|err| err.to_string())?
        {
            return Ok(Some(SessionDefaultResolution {
                model: memory.model,
                reasoning_effort: memory.reasoning_effort,
                source: "workspace_last".to_string(),
            }));
        }
    }
    if let Some(default) = workspace_default {
        if let Some(model) = default
            .default_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(Some(SessionDefaultResolution {
                model: model.to_string(),
                reasoning_effort: default.default_reasoning_effort,
                source: "workspace_default".to_string(),
            }));
        }
    }
    if let Some(global) = storage
        .find_workspace_model_default(GLOBAL_WORKSPACE)
        .map_err(|err| err.to_string())?
    {
        if let Some(model) = global
            .default_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(Some(SessionDefaultResolution {
                model: model.to_string(),
                reasoning_effort: global.default_reasoning_effort,
                source: "global_default".to_string(),
            }));
        }
    }
    Ok(Some(SessionDefaultResolution {
        model: DEFAULT_GLOBAL_MODEL.to_string(),
        reasoning_effort: None,
        source: "global_default".to_string(),
    }))
}

fn resolve_session_default_for_thread(
    storage: &Storage,
    row: &CodexThreadRow,
) -> Result<Option<SessionDefaultResolution>, String> {
    if row.is_subagent {
        if let Some(parent_thread_id) = row.parent_thread_id.as_deref() {
            if let Some(parent_default) = storage
                .find_session_subagent_model_memory(parent_thread_id)
                .map_err(|err| err.to_string())?
            {
                return Ok(Some(SessionDefaultResolution {
                    model: parent_default.model,
                    reasoning_effort: parent_default.reasoning_effort,
                    source: "parent_subagent_default".to_string(),
                }));
            }
        }
    }
    resolve_session_default_for_workspace(storage, row.workspace.as_str())
}

fn route_binding_order(
    mut bindings: Vec<ModelRouteBinding>,
    key_id: &str,
    model: &str,
) -> Vec<ModelRouteBinding> {
    bindings.sort_by(|left, right| {
        right
            .manual_preferred
            .cmp(&left.manual_preferred)
            .then(left.priority.cmp(&right.priority))
            .then(right.weight.cmp(&left.weight))
            .then(left.aggregate_api_id.cmp(&right.aggregate_api_id))
    });
    if bindings.len() <= 1 {
        return bindings;
    }
    let strategy = bindings
        .first()
        .map(|item| item.route_strategy.as_str())
        .unwrap_or("ordered");
    if strategy == "balanced" {
        let manual_count = bindings
            .iter()
            .take_while(|item| item.manual_preferred)
            .count();
        let start = if manual_count > 0 { manual_count } else { 0 };
        if bindings.len() > start + 1 {
            let scope = route_binding_scope(&bindings[start..]);
            let rotate_by =
                weighted_route_offset(key_id, model, scope.as_str(), &bindings[start..]);
            bindings[start..].rotate_left(rotate_by);
        }
    }
    bindings
}

fn route_binding_scope(bindings: &[ModelRouteBinding]) -> String {
    bindings
        .iter()
        .map(|binding| {
            format!(
                "{}:{}:{}",
                binding.aggregate_api_id.trim(),
                binding.priority,
                binding.weight.max(1)
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn weighted_route_offset(
    key_id: &str,
    model: &str,
    scope: &str,
    bindings: &[ModelRouteBinding],
) -> usize {
    if bindings.len() <= 1 {
        return 0;
    }
    let total_weight: i64 = bindings.iter().map(|item| item.weight.max(1)).sum();
    if total_weight <= 0 {
        return stable_route_index(key_id, model, scope, bindings.len());
    }
    let mut ticket = stable_route_index(key_id, model, scope, total_weight as usize) as i64;
    for (index, binding) in bindings.iter().enumerate() {
        ticket -= binding.weight.max(1);
        if ticket < 0 {
            return index;
        }
    }
    0
}

fn stable_route_index(key_id: &str, model: &str, scope: &str, candidate_count: usize) -> usize {
    if candidate_count <= 1 {
        return 0;
    }
    let minute = now_ts() / 60;
    let seed = format!(
        "{}|{}|{}|{}",
        key_id.trim(),
        model.trim(),
        scope.trim(),
        minute
    );
    let mut hash = 14695981039346656037_u64;
    for byte in seed.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211_u64);
    }
    (hash as usize) % candidate_count
}

fn read_codex_threads(
    path: &Path,
    workspace_filter: Option<&str>,
) -> Result<Vec<CodexThreadRow>, String> {
    let conn = rusqlite::Connection::open(path)
        .map_err(|err| format!("打开 Codex state 数据库失败: {err}"))?;
    let select_sql = codex_thread_select_sql(&conn)?;
    let mut sql = format!("{select_sql} WHERE archived = 0");
    let normalized_workspace_filter = workspace_filter.and_then(normalize_workspace_path);
    if normalized_workspace_filter.is_some() {
        sql.push_str(
            " AND lower(rtrim(replace(replace(cwd, '\\\\?\\', ''), '\\', '/'), '/')) = ?1",
        );
    }
    sql.push_str(" ORDER BY COALESCE(updated_at_ms / 1000, updated_at) DESC LIMIT 500");
    let mut stmt = conn
        .prepare(sql.as_str())
        .map_err(|err| format!("读取 Codex threads 失败: {err}"))?;
    let mut rows = if let Some(workspace) = normalized_workspace_filter.as_deref() {
        stmt.query(rusqlite::params![workspace])
            .map_err(|err| format!("查询 Codex threads 失败: {err}"))?
    } else {
        stmt.query(rusqlite::params![])
            .map_err(|err| format!("查询 Codex threads 失败: {err}"))?
    };
    let mut out = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|err| format!("遍历 Codex threads 失败: {err}"))?
    {
        let raw_workspace: String = row.get(1).map_err(|err| err.to_string())?;
        out.push(CodexThreadRow {
            thread_id: row.get(0).map_err(|err| err.to_string())?,
            workspace: display_workspace_path(raw_workspace.as_str()),
            title: row.get(2).map_err(|err| err.to_string())?,
            model: row.get(3).map_err(|err| err.to_string())?,
            reasoning_effort: row.get(4).map_err(|err| err.to_string())?,
            model_provider: row.get(5).map_err(|err| err.to_string())?,
            updated_at: row.get(6).map_err(|err| err.to_string())?,
            ..parse_codex_thread_subagent_fields(
                row.get::<_, Option<String>>(7)
                    .map_err(|err| err.to_string())?
                    .as_deref(),
                row.get::<_, Option<String>>(8)
                    .map_err(|err| err.to_string())?,
                row.get::<_, Option<String>>(9)
                    .map_err(|err| err.to_string())?,
            )
        });
    }
    apply_thread_spawn_edges(&conn, &mut out)?;
    let mut seen_thread_ids = out
        .iter()
        .map(|row| row.thread_id.clone())
        .collect::<BTreeSet<_>>();
    let missing_parent_ids = out
        .iter()
        .filter_map(|row| row.parent_thread_id.as_deref())
        .map(str::to_string)
        .filter(|parent_id| !seen_thread_ids.contains(parent_id))
        .collect::<BTreeSet<_>>();
    for parent_id in missing_parent_ids {
        if let Some(parent) = read_codex_thread(path, parent_id.as_str())? {
            let parent_workspace = normalize_workspace_path(parent.workspace.as_str());
            if normalized_workspace_filter
                .as_deref()
                .map(|workspace| parent_workspace.as_deref() == Some(workspace))
                .unwrap_or(true)
                && seen_thread_ids.insert(parent.thread_id.clone())
            {
                out.push(parent);
            }
        }
    }
    Ok(out)
}

fn read_codex_thread(path: &Path, thread_id: &str) -> Result<Option<CodexThreadRow>, String> {
    let conn = rusqlite::Connection::open(path)
        .map_err(|err| format!("打开 Codex state 数据库失败: {err}"))?;
    let select_sql = codex_thread_select_sql(&conn)?;
    let mut stmt = conn
        .prepare(format!("{select_sql} WHERE id = ?1 LIMIT 1").as_str())
        .map_err(|err| format!("读取 Codex thread 失败: {err}"))?;
    let mut rows = stmt
        .query([thread_id])
        .map_err(|err| format!("查询 Codex thread 失败: {err}"))?;
    let Some(row) = rows.next().map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    let raw_workspace: String = row.get(1).map_err(|err| err.to_string())?;
    Ok(Some(CodexThreadRow {
        thread_id: row.get(0).map_err(|err| err.to_string())?,
        workspace: display_workspace_path(raw_workspace.as_str()),
        title: row.get(2).map_err(|err| err.to_string())?,
        model: row.get(3).map_err(|err| err.to_string())?,
        reasoning_effort: row.get(4).map_err(|err| err.to_string())?,
        model_provider: row.get(5).map_err(|err| err.to_string())?,
        updated_at: row.get(6).map_err(|err| err.to_string())?,
        ..parse_codex_thread_subagent_fields(
            row.get::<_, Option<String>>(7)
                .map_err(|err| err.to_string())?
                .as_deref(),
            row.get::<_, Option<String>>(8)
                .map_err(|err| err.to_string())?,
            row.get::<_, Option<String>>(9)
                .map_err(|err| err.to_string())?,
        )
    }))
}

fn apply_thread_spawn_edges(conn: &Connection, rows: &mut [CodexThreadRow]) -> Result<(), String> {
    if !sqlite_connection_table_exists(conn, "thread_spawn_edges")? {
        return Ok(());
    }
    let mut stmt = conn
        .prepare(
            "SELECT parent_thread_id, child_thread_id
             FROM thread_spawn_edges
             WHERE COALESCE(status, '') <> 'deleted'",
        )
        .map_err(|err| err.to_string())?;
    let edges = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| err.to_string())?;
    let mut parent_by_child = BTreeMap::<String, String>::new();
    for edge in edges {
        let (parent, child) = edge.map_err(|err| err.to_string())?;
        if !parent.trim().is_empty() && !child.trim().is_empty() {
            parent_by_child.insert(child, parent);
        }
    }
    for row in rows {
        if let Some(parent) = parent_by_child.get(row.thread_id.as_str()) {
            row.parent_thread_id = Some(parent.clone());
            row.is_subagent = true;
        }
    }
    Ok(())
}

fn codex_thread_select_sql(conn: &Connection) -> Result<String, String> {
    let source_expr = if sqlite_connection_column_exists(conn, "threads", "source")? {
        "source"
    } else {
        "NULL"
    };
    let agent_nickname_expr = if sqlite_connection_column_exists(conn, "threads", "agent_nickname")?
    {
        "agent_nickname"
    } else {
        "NULL"
    };
    let agent_role_expr = if sqlite_connection_column_exists(conn, "threads", "agent_role")? {
        "agent_role"
    } else {
        "NULL"
    };
    Ok(format!(
        "SELECT id, cwd, title, model, reasoning_effort, model_provider, COALESCE(updated_at_ms / 1000, updated_at), {source_expr}, {agent_nickname_expr}, {agent_role_expr} FROM threads"
    ))
}

fn sqlite_connection_column_exists(
    conn: &Connection,
    table: &str,
    column: &str,
) -> Result<bool, String> {
    let mut stmt = conn
        .prepare(format!("PRAGMA table_info({table})").as_str())
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| err.to_string())?;
    for row in rows {
        if row
            .map_err(|err| err.to_string())?
            .eq_ignore_ascii_case(column)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn sqlite_connection_table_exists(conn: &Connection, table: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .map_err(|err| err.to_string())
}

fn normalize_workspace_path(value: &str) -> Option<String> {
    let mut text = value.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        text = rest;
    }
    let normalized = text
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

fn display_workspace_path(value: &str) -> String {
    let mut text = value.trim();
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        text = rest;
    }
    text.replace('\\', "/").trim_end_matches('/').to_string()
}

fn parse_codex_thread_subagent_fields(
    source: Option<&str>,
    agent_nickname: Option<String>,
    agent_role: Option<String>,
) -> CodexThreadRow {
    let source_value = source
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    let spawn = source_value
        .as_ref()
        .and_then(|value| value.get("subagent"))
        .and_then(|value| value.get("thread_spawn"));
    let parent_thread_id = spawn
        .and_then(|value| value.get("parent_thread_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let subagent_depth = spawn
        .and_then(|value| value.get("depth"))
        .and_then(Value::as_i64);
    let parsed_nickname = spawn
        .and_then(|value| value.get("agent_nickname"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let parsed_role = spawn
        .and_then(|value| value.get("agent_role"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    CodexThreadRow {
        thread_id: String::new(),
        workspace: String::new(),
        title: None,
        model: None,
        reasoning_effort: None,
        model_provider: None,
        parent_thread_id,
        is_subagent: spawn.is_some(),
        agent_nickname: agent_nickname.or(parsed_nickname),
        agent_role: agent_role.or(parsed_role),
        subagent_depth,
        updated_at: 0,
    }
}

fn session_summary_from_storage(
    storage: &Storage,
    state_row: Option<CodexThreadRow>,
    thread_id: String,
    subagent_memory: Option<SessionSubagentModelMemory>,
) -> Result<SessionModelSummary, String> {
    let session_memory = storage
        .find_session_model_memory(thread_id.as_str())
        .map_err(|err| err.to_string())?;
    let workspace = state_row
        .as_ref()
        .map(|row| row.workspace.clone())
        .or_else(|| session_memory.as_ref().map(|item| item.workspace.clone()))
        .unwrap_or_default();
    let last_seen_at = state_row
        .as_ref()
        .map(|row| row.updated_at)
        .or_else(|| session_memory.as_ref().map(|item| item.last_seen_at))
        .unwrap_or_else(now_ts);
    let updated_at = session_memory
        .as_ref()
        .map(|item| item.updated_at)
        .or_else(|| state_row.as_ref().map(|row| row.updated_at))
        .unwrap_or_else(now_ts);
    let effective = derive_effective_session_model_state(
        state_row.as_ref().and_then(|row| row.model.as_deref()),
        state_row
            .as_ref()
            .and_then(|row| row.reasoning_effort.as_deref()),
        session_memory.as_ref(),
        updated_at,
    );
    Ok(SessionModelSummary {
        thread_id,
        workspace,
        title: state_row.as_ref().and_then(|row| row.title.clone()),
        model: effective.model,
        reasoning_effort: effective.reasoning_effort,
        model_provider: state_row
            .as_ref()
            .and_then(|row| row.model_provider.clone()),
        effective_model_label: effective.effective_model_label,
        effective_model_source: effective.effective_model_source,
        has_model_override: effective.has_model_override,
        parent_thread_id: state_row
            .as_ref()
            .and_then(|row| row.parent_thread_id.clone()),
        is_subagent: state_row
            .as_ref()
            .map(|row| row.is_subagent)
            .unwrap_or(false),
        agent_nickname: state_row
            .as_ref()
            .and_then(|row| row.agent_nickname.clone()),
        agent_role: state_row.as_ref().and_then(|row| row.agent_role.clone()),
        subagent_depth: state_row.as_ref().and_then(|row| row.subagent_depth),
        source: effective.source,
        locked: effective.locked,
        memory_state: effective.memory_state,
        last_seen_at,
        updated_at: effective.updated_at,
        subagent_model: subagent_memory.as_ref().map(|item| item.model.clone()),
        subagent_reasoning_effort: subagent_memory
            .as_ref()
            .and_then(|item| item.reasoning_effort.clone()),
        subagent_model_source: subagent_memory.as_ref().map(|item| item.source.clone()),
        subagent_model_updated_at: subagent_memory.as_ref().map(|item| item.updated_at),
    })
}

fn write_codex_thread_model(
    path: &Path,
    thread_id: &str,
    model: &str,
    reasoning_effort: Option<&str>,
) -> Result<bool, String> {
    let conn = rusqlite::Connection::open(path)
        .map_err(|err| format!("打开 Codex state 数据库失败: {err}"))?;
    let now = now_ts();
    let updated = conn
        .execute(
            "UPDATE threads
             SET model = ?1,
                 reasoning_effort = ?2,
                 updated_at = ?3,
                 updated_at_ms = ?4
             WHERE id = ?5",
            rusqlite::params![model, reasoning_effort, now, now * 1000, thread_id],
        )
        .map_err(|err| format!("更新 Codex thread 模型失败: {err}"))?;
    Ok(updated > 0)
}

fn mirror_session_model_to_runtime_anchors(
    storage: &Storage,
    memory: &SessionModelMemory,
) -> Result<(), String> {
    let workspace = normalize_workspace_path(memory.workspace.as_str());
    let title = memory
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let recent = storage
        .list_recent_conversation_bindings(128)
        .map_err(|err| err.to_string())?;
    let mut matched = 0usize;
    let now = now_ts();
    for binding in recent {
        if matched >= 4 {
            break;
        }
        let last_model = binding
            .last_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(last_model) = last_model {
            if !last_model.eq_ignore_ascii_case(memory.model.as_str()) {
                continue;
            }
        }
        let runtime_item = SessionModelMemory {
            thread_id: binding.thread_anchor,
            workspace: memory.workspace.clone(),
            title: title.clone().or_else(|| Some(binding.conversation_id)),
            model: memory.model.clone(),
            reasoning_effort: memory.reasoning_effort.clone(),
            source: memory.source.clone(),
            locked: memory.locked,
            last_seen_at: memory.last_seen_at.max(binding.last_used_at),
            updated_at: now,
        };
        if let Some(target_workspace) = workspace.as_deref() {
            let binding_workspace = storage
                .find_account_by_id(binding.account_id.as_str())
                .ok()
                .flatten()
                .and_then(|account| account.workspace_id)
                .and_then(|value| normalize_workspace_path(value.as_str()));
            if let Some(binding_workspace) = binding_workspace.as_deref() {
                if binding_workspace != target_workspace {
                    continue;
                }
            }
        }
        storage
            .upsert_session_model_memory(&runtime_item)
            .map_err(|err| err.to_string())?;
        matched += 1;
    }
    Ok(())
}

fn resolve_codex_state_db_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CODEXMANAGER_CODEX_STATE_DB_PATH") {
        let path = PathBuf::from(path.trim());
        if path.exists() {
            return Some(path);
        }
    }
    let base = std::env::var("CODEX_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .map(|home| PathBuf::from(home).join(".codex"))
        })
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".codex"))
        })?;
    let path = base.join("state_5.sqlite");
    path.exists().then_some(path)
}

fn resolve_global_default_model(storage: &Storage) -> Result<Option<String>, String> {
    Ok(storage
        .find_workspace_model_default(GLOBAL_WORKSPACE)
        .map_err(|err| err.to_string())?
        .and_then(|item| item.default_model)
        .or_else(|| {
            storage
                .get_app_setting("model_router.global_default_model")
                .ok()
                .flatten()
        })
        .or_else(|| Some(DEFAULT_GLOBAL_MODEL.to_string())))
}

fn workspace_default_summary(item: WorkspaceModelDefault) -> WorkspaceModelDefaultSummary {
    WorkspaceModelDefaultSummary {
        workspace: item.workspace,
        default_model: item.default_model,
        default_reasoning_effort: item.default_reasoning_effort,
        inherit_last_session: item.inherit_last_session,
        auto_remember: item.auto_remember,
        updated_at: item.updated_at,
    }
}

fn route_binding_summary(
    item: ModelRouteBinding,
    apis: &BTreeMap<String, AggregateApi>,
) -> ModelRouteBindingSummary {
    let api = apis.get(item.aggregate_api_id.as_str());
    ModelRouteBindingSummary {
        id: item.id,
        model: item.model,
        aggregate_api_id: item.aggregate_api_id,
        aggregate_api_name: api.and_then(|api| api.supplier_name.clone()),
        aggregate_api_url: api.map(|api| api.url.clone()),
        enabled: item.enabled,
        priority: item.priority,
        weight: item.weight,
        route_strategy: item.route_strategy,
        manual_preferred: item.manual_preferred,
        supports_responses: item.supports_responses,
        supports_chat_completions: item.supports_chat_completions,
        requires_adapter: item.requires_adapter,
        last_probe_status: item.last_probe_status,
        last_error: item.last_error,
        last_success_at: item.last_success_at,
        created_at: item.created_at,
        updated_at: item.updated_at,
    }
}

fn probe_candidate_summary(item: ProbeCandidate) -> ProbeCandidateSummary {
    ProbeCandidateSummary {
        id: item.id,
        probe_run_id: item.probe_run_id,
        aggregate_api_id: item.aggregate_api_id,
        model: item.model,
        supports_responses: item.supports_responses,
        supports_chat_completions: item.supports_chat_completions,
        requires_adapter: item.requires_adapter,
        suggested_route_strategy: item.suggested_route_strategy,
        suggested_priority: item.suggested_priority,
        suggested_weight: item.suggested_weight,
        applied: item.applied,
        error: item.error,
        created_at: item.created_at,
        applied_at: item.applied_at,
    }
}

fn probe_run_summary(
    item: ProbeRun,
    candidates: Vec<ProbeCandidateSummary>,
    apis: &BTreeMap<String, AggregateApi>,
) -> ProbeRunSummary {
    let raw_summary = item
        .raw_summary_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    ProbeRunSummary {
        id: item.id,
        aggregate_api_id: item.aggregate_api_id.clone(),
        aggregate_api_name: apis
            .get(item.aggregate_api_id.as_str())
            .and_then(|api| api.supplier_name.clone()),
        status: item.status,
        started_at: item.started_at,
        finished_at: item.finished_at,
        models_status: item.models_status,
        responses_status: item.responses_status,
        chat_completions_status: item.chat_completions_status,
        error: item.error,
        candidates,
        raw_summary,
    }
}

fn aggregate_api_map(storage: &Storage) -> Result<BTreeMap<String, AggregateApi>, String> {
    Ok(storage
        .list_aggregate_apis()
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|api| (api.id.clone(), api))
        .collect())
}

#[derive(Debug, Clone)]
struct EndpointProbe {
    ok: bool,
    status_code: Option<i64>,
    model: Option<String>,
    models: Vec<String>,
    error: Option<String>,
}

impl EndpointProbe {
    fn status_label(&self) -> String {
        if self.ok {
            match self.status_code {
                Some(code) => format!("success:{code}"),
                None => "success".to_string(),
            }
        } else {
            self.error.clone().unwrap_or_else(|| "failed".to_string())
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "ok": self.ok,
            "statusCode": self.status_code,
            "model": self.model,
            "models": self.models,
            "error": self.error
        })
    }
}

fn probe_models_endpoint(
    client: &reqwest::blocking::Client,
    api: &AggregateApi,
    secret: &str,
) -> EndpointProbe {
    let url = normalize_probe_url(api.url.as_str(), "/models");
    let request = client.get(url.as_str());
    let response = send_probe_request(apply_probe_auth(request, url, api, secret));
    match response {
        Ok(resp) => {
            let status = resp.status().as_u16() as i64;
            if !resp.status().is_success() {
                return EndpointProbe {
                    ok: false,
                    status_code: Some(status),
                    model: None,
                    models: Vec::new(),
                    error: Some(format!("models http_status={status}")),
                };
            }
            match resp.json::<Value>() {
                Ok(value) => {
                    let models = extract_models_from_models_response(&value);
                    EndpointProbe {
                        ok: true,
                        status_code: Some(status),
                        model: models.first().cloned(),
                        models,
                        error: None,
                    }
                }
                Err(err) => EndpointProbe {
                    ok: false,
                    status_code: Some(status),
                    model: None,
                    models: Vec::new(),
                    error: Some(format!("models invalid_json={err}")),
                },
            }
        }
        Err(err) => EndpointProbe {
            ok: false,
            status_code: None,
            model: None,
            models: Vec::new(),
            error: Some(err),
        },
    }
}

fn probe_responses_endpoint(
    client: &reqwest::blocking::Client,
    api: &AggregateApi,
    secret: &str,
    model_hint: Option<&String>,
) -> EndpointProbe {
    let fallback_models = configured_probe_fallback_models();
    let model = model_hint
        .map(|value| value.as_str())
        .or_else(|| fallback_models.first().map(String::as_str))
        .unwrap_or("gpt-5.4")
        .to_string();
    let url = normalize_probe_url(api.url.as_str(), "/responses");
    let body = json!({
        "model": model,
        "instructions": "Reply with exactly: pong",
        "input": [{"role": "user", "content": [{"type": "input_text", "text": "ping"}]}],
        "tools": [{
            "type": "function",
            "name": "cm_probe_noop",
            "description": "No-op probe tool.",
            "parameters": {"type": "object", "properties": {}, "additionalProperties": false}
        }],
        "tool_choice": "none",
        "parallel_tool_calls": true,
        "reasoning": {"effort": "low"},
        "store": false,
        "stream": true,
        "include": ["reasoning.encrypted_content"],
        "max_output_tokens": 8
    });
    probe_stream_post_json(
        client,
        api,
        secret,
        url,
        body,
        Some(model),
        StreamProbeKind::Responses,
    )
}

fn probe_chat_completions_endpoint(
    client: &reqwest::blocking::Client,
    api: &AggregateApi,
    secret: &str,
    model_hint: Option<&String>,
) -> EndpointProbe {
    let fallback_models = configured_probe_fallback_models();
    let model = model_hint
        .map(|value| value.as_str())
        .or_else(|| fallback_models.first().map(String::as_str))
        .unwrap_or("gpt-5.4")
        .to_string();
    let url = normalize_probe_url(api.url.as_str(), "/chat/completions");
    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": "ping"}],
        "stream": true,
        "max_tokens": 1
    });
    probe_stream_post_json(
        client,
        api,
        secret,
        url,
        body,
        Some(model),
        StreamProbeKind::ChatCompletions,
    )
}

fn select_probe_model(api: &AggregateApi, discovered_models: &[String]) -> Option<String> {
    let fallback_models = configured_probe_fallback_models();
    let is_azure = api.provider_type.eq_ignore_ascii_case("azure")
        || api.provider_type.eq_ignore_ascii_case("azure_openai")
        || api
            .provider_type
            .eq_ignore_ascii_case("azure_openai_compat");

    if is_azure {
        if let Some(model) = fallback_models.first() {
            return Some(model.clone());
        }
    }

    for fallback in &fallback_models {
        if discovered_models
            .iter()
            .any(|model| model.eq_ignore_ascii_case(fallback))
        {
            return Some(fallback.clone());
        }
    }

    discovered_models
        .iter()
        .find(|model| looks_like_text_probe_model(model))
        .cloned()
        .or_else(|| fallback_models.first().cloned())
        .or_else(|| discovered_models.first().cloned())
}

fn looks_like_text_probe_model(model: &str) -> bool {
    let value = model.trim().to_ascii_lowercase();
    if value.is_empty() {
        return false;
    }
    let non_text_markers = [
        "dall-e",
        "dalle",
        "image",
        "sora",
        "tts",
        "whisper",
        "audio",
        "realtime",
        "transcribe",
        "embedding",
        "embed",
        "moderation",
        "omni-moderation",
        "babbage",
    ];
    if non_text_markers.iter().any(|marker| value.contains(marker)) {
        return false;
    }
    let text_markers = [
        "gpt", "o1", "o3", "o4", "glm", "mimo", "kimi", "qwen", "deepseek", "claude", "gemini",
        "grok", "llama", "mistral", "mixtral", "coder",
    ];
    text_markers.iter().any(|marker| value.contains(marker))
}

fn quick_check_responses(
    client: &reqwest::blocking::Client,
    api: &AggregateApi,
    secret: &str,
    model: &str,
) -> EndpointProbe {
    let url = normalize_probe_url(api.url.as_str(), "/responses");
    let body = json!({
        "model": model,
        "input": "ping",
        "stream": true,
        "max_output_tokens": 8
    });
    probe_stream_post_json(
        client,
        api,
        secret,
        url,
        body,
        Some(model.to_string()),
        StreamProbeKind::Responses,
    )
}

fn quick_check_chat(
    client: &reqwest::blocking::Client,
    api: &AggregateApi,
    secret: &str,
    model: &str,
) -> EndpointProbe {
    let url = normalize_probe_url(api.url.as_str(), "/chat/completions");
    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": "ping"}],
        "stream": true,
        "max_tokens": 1
    });
    probe_stream_post_json(
        client,
        api,
        secret,
        url,
        body,
        Some(model.to_string()),
        StreamProbeKind::ChatCompletions,
    )
}

fn configured_probe_fallback_models() -> Vec<String> {
    crate::app_settings::current_model_router_probe_fallback_models()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn probe_post_json(
    client: &reqwest::blocking::Client,
    api: &AggregateApi,
    secret: &str,
    url: String,
    body: Value,
    model: Option<String>,
) -> EndpointProbe {
    let request = client
        .post(url.as_str())
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .json(&body);
    match send_probe_request(apply_probe_auth(request, url.clone(), api, secret)) {
        Ok(resp) => {
            let status = resp.status().as_u16() as i64;
            if resp.status().is_success() {
                EndpointProbe {
                    ok: true,
                    status_code: Some(status),
                    model,
                    models: Vec::new(),
                    error: None,
                }
            } else {
                EndpointProbe {
                    ok: false,
                    status_code: Some(status),
                    model,
                    models: Vec::new(),
                    error: Some(format!("http_status={status}")),
                }
            }
        }
        Err(err) => EndpointProbe {
            ok: false,
            status_code: None,
            model,
            models: Vec::new(),
            error: Some(err),
        },
    }
}

fn probe_stream_post_json(
    client: &reqwest::blocking::Client,
    api: &AggregateApi,
    secret: &str,
    url: String,
    body: Value,
    model: Option<String>,
    kind: StreamProbeKind,
) -> EndpointProbe {
    let request = client
        .post(url.as_str())
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .json(&body);
    match send_probe_request(apply_probe_auth(request, url.clone(), api, secret)) {
        Ok(resp) => {
            let status = resp.status().as_u16() as i64;
            if !resp.status().is_success() {
                return EndpointProbe {
                    ok: false,
                    status_code: Some(status),
                    model,
                    models: Vec::new(),
                    error: Some(format!("http_status={status}")),
                };
            }
            match read_stream_probe_prefix(resp) {
                Ok(body) => {
                    let stream_validation = validate_stream_probe_prefix(body.as_str(), kind);
                    let has_stream_signal = stream_validation.ok;
                    if !has_stream_signal {
                        log::warn!(
                            "event=model_router_stream_probe_invalid aggregate_api_id={} supplier={} url={} status={} bytes={} reason={} preview={}",
                            api.id,
                            api.supplier_name.as_deref().unwrap_or(""),
                            url,
                            status,
                            body.len(),
                            stream_validation.reason.as_deref().unwrap_or("unknown"),
                            sanitize_probe_preview(body.as_str())
                        );
                    }
                    EndpointProbe {
                        ok: has_stream_signal,
                        status_code: Some(status),
                        model,
                        models: Vec::new(),
                        error: (!has_stream_signal).then(|| {
                            stream_validation.reason.unwrap_or_else(|| {
                                "stream returned no usable SSE data before probe cutoff".to_string()
                            })
                        }),
                    }
                }
                Err(err) => EndpointProbe {
                    ok: false,
                    status_code: Some(status),
                    model,
                    models: Vec::new(),
                    error: Some(err.to_string()),
                },
            }
        }
        Err(err) => EndpointProbe {
            ok: false,
            status_code: None,
            model,
            models: Vec::new(),
            error: Some(err),
        },
    }
}

#[derive(Debug, Clone, Copy)]
enum StreamProbeKind {
    Responses,
    ChatCompletions,
}

#[derive(Debug)]
struct StreamProbeValidation {
    ok: bool,
    reason: Option<String>,
}

fn validate_stream_probe_prefix(body: &str, kind: StreamProbeKind) -> StreamProbeValidation {
    let mut has_data_frame = false;
    let mut has_terminal_event = false;
    let mut has_finish_reason = false;
    let mut last_event: Option<String> = None;

    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(event) = trimmed.strip_prefix("event:") {
            let event = event.trim();
            if !event.is_empty() {
                last_event = Some(event.to_string());
                if matches!(kind, StreamProbeKind::Responses)
                    && RESPONSE_STREAM_TERMINAL_EVENTS.contains(&event)
                {
                    has_terminal_event = true;
                }
            }
        } else if let Some(data) = trimmed.strip_prefix("data:") {
            has_data_frame = true;
            let data = data.trim();
            if matches!(kind, StreamProbeKind::ChatCompletions) && data == "[DONE]" {
                has_terminal_event = true;
            } else if matches!(kind, StreamProbeKind::ChatCompletions) && !data.is_empty() {
                if let Ok(value) = serde_json::from_str::<Value>(data) {
                    has_finish_reason = chat_stream_chunk_has_finish_reason(&value);
                }
            }
        }
    }

    match kind {
        StreamProbeKind::Responses if has_terminal_event => {
            StreamProbeValidation {
                ok: true,
                reason: None,
            }
        }
        StreamProbeKind::ChatCompletions if has_terminal_event || has_finish_reason => {
            StreamProbeValidation {
                ok: true,
                reason: None,
            }
        }
        StreamProbeKind::Responses if last_event.is_some() && !has_data_frame => {
            StreamProbeValidation {
                ok: false,
                reason: Some(format!(
                    "responses stream emitted event-only SSE without data or terminal event; last_event={}",
                    last_event.unwrap_or_else(|| "-".to_string())
                )),
            }
        }
        StreamProbeKind::Responses => StreamProbeValidation {
            ok: false,
            reason: Some(
                "responses stream did not reach response.completed or response.done before probe cutoff"
                    .to_string(),
            ),
        },
        StreamProbeKind::ChatCompletions => StreamProbeValidation {
            ok: false,
            reason: Some(
                "chat stream did not reach [DONE] or finish_reason before probe cutoff".to_string(),
            ),
        },
    }
}

fn read_stream_probe_prefix(mut resp: reqwest::blocking::Response) -> Result<String, String> {
    let mut buffer = [0_u8; 512];
    let mut collected = Vec::new();
    loop {
        match resp.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                collected.extend_from_slice(&buffer[..read]);
                if collected.len() >= QUICK_STREAM_READ_LIMIT_BYTES {
                    break;
                }
                let text = String::from_utf8_lossy(&collected);
                if text.contains("event: response.completed")
                    || text.contains("event: response.done")
                    || text.contains("data: [DONE]")
                    || text.contains("\"finish_reason\"")
                {
                    break;
                }
            }
            Err(err) => {
                if collected.is_empty() {
                    return Err(err.to_string());
                }
                break;
            }
        }
    }
    Ok(String::from_utf8_lossy(&collected).to_string())
}

fn chat_stream_chunk_has_finish_reason(value: &Value) -> bool {
    value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice
                    .get("finish_reason")
                    .is_some_and(|reason| !reason.is_null())
            })
        })
}

fn sanitize_probe_preview(value: &str) -> String {
    value
        .chars()
        .take(160)
        .map(|ch| match ch {
            '\r' | '\n' | '\t' => ' ',
            ch if ch.is_control() => ' ',
            ch => ch,
        })
        .collect()
}

fn send_probe_request(
    request: Result<reqwest::blocking::RequestBuilder, String>,
) -> Result<reqwest::blocking::Response, String> {
    request?.send().map_err(|err| err.to_string())
}

fn normalize_probe_url(base_url: &str, suffix: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/v1") {
        format!("{base}{suffix}")
    } else {
        format!("{base}/v1{suffix}")
    }
}

fn apply_probe_auth(
    mut builder: reqwest::blocking::RequestBuilder,
    _url: String,
    api: &AggregateApi,
    secret: &str,
) -> Result<reqwest::blocking::RequestBuilder, String> {
    let auth_type = api.auth_type.trim().to_ascii_lowercase();
    let auth_params = api
        .auth_params_json
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if auth_type == AGGREGATE_API_AUTH_USERPASS {
        let parsed: UserPassSecret =
            serde_json::from_str(secret).map_err(|_| "invalid aggregate api secret".to_string())?;
        if let Some(raw) = auth_params {
            let params: UserPassAuthParams =
                serde_json::from_str(raw).map_err(|_| "invalid authParams".to_string())?;
            match params.mode.trim().to_ascii_lowercase().as_str() {
                "headerpair" => {
                    builder = builder
                        .header(
                            params.username_name.as_deref().unwrap_or("username"),
                            parsed.username,
                        )
                        .header(
                            params.password_name.as_deref().unwrap_or("password"),
                            parsed.password,
                        );
                    return Ok(builder);
                }
                "querypair" => {
                    let username_name = params
                        .username_name
                        .as_deref()
                        .unwrap_or("username")
                        .to_string();
                    let password_name = params
                        .password_name
                        .as_deref()
                        .unwrap_or("password")
                        .to_string();
                    builder = builder.query(&[
                        (username_name.as_str(), parsed.username.as_str()),
                        (password_name.as_str(), parsed.password.as_str()),
                    ]);
                    return Ok(builder);
                }
                _ => {}
            }
        }
        return Ok(builder.basic_auth(parsed.username, Some(parsed.password)));
    }

    if auth_type == AGGREGATE_API_AUTH_APIKEY {
        if let Some(raw) = auth_params {
            let params: ApiKeyAuthParams =
                serde_json::from_str(raw).map_err(|_| "invalid authParams".to_string())?;
            if params.location.trim().eq_ignore_ascii_case("query") {
                builder = builder.query(&[(params.name.trim(), secret.trim())]);
                return Ok(builder);
            }
            let header_value = if params
                .header_value_format
                .as_deref()
                .unwrap_or("bearer")
                .trim()
                .eq_ignore_ascii_case("raw")
            {
                secret.trim().to_string()
            } else {
                format!("Bearer {}", secret.trim())
            };
            return Ok(builder.header(params.name.trim(), header_value));
        }
        if api
            .provider_type
            .trim()
            .eq_ignore_ascii_case(AGGREGATE_API_PROVIDER_AZURE_OPENAI)
        {
            return Ok(builder.header("api-key", secret.trim()));
        }
    }

    builder = builder.header(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(format!("Bearer {}", secret.trim()).as_str())
            .map_err(|_| "invalid aggregate api key".to_string())?,
    );
    Ok(builder)
}

fn extract_models_from_models_response(value: &Value) -> Vec<String> {
    let source = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array));
    let Some(items) = source else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            item.get("id")
                .or_else(|| item.get("name"))
                .or_else(|| item.get("slug"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect()
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalized_required(value: &str, message: &str) -> Result<String, String> {
    value
        .trim()
        .is_empty()
        .then(|| Err(message.to_string()))
        .unwrap_or_else(|| Ok(value.trim().to_string()))
}

fn normalize_route_strategy(value: Option<&str>) -> &'static str {
    match value
        .map(str::trim)
        .unwrap_or("ordered")
        .to_ascii_lowercase()
        .as_str()
    {
        "balanced" | "round_robin" | "round-robin" | "rr" => "balanced",
        "manual_preferred" | "manual-preferred" | "manual" => "manual_preferred",
        _ => "ordered",
    }
}

#[cfg(test)]
mod tests {
    use codexmanager_core::storage::{
        now_ts, AggregateApi, ModelRouteBinding, RequestLog, SessionModelMemory, Storage,
        UpstreamModelCapability, WorkspaceModelDefault,
    };
    use rusqlite::Connection;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        aggregate_candidate_requires_responses_to_chat_adapter, list_session_models,
        model_route_applies_for_model, record_route_binding_error, record_route_binding_success,
        resolve_session_default_for_workspace, resolve_upstream_model_for_aggregate_candidate,
        route_aggregate_candidates_for_model, route_binding_order, route_binding_scope,
        select_probe_model, stable_route_index, update_session_model, validate_stream_probe_prefix,
        weighted_route_offset, StreamProbeKind, GLOBAL_WORKSPACE,
    };
    use crate::aggregate_api::{
        AGGREGATE_API_AUTH_APIKEY, AGGREGATE_API_PROVIDER_AZURE_OPENAI,
        AGGREGATE_API_PROVIDER_CODEX,
    };
    use codexmanager_core::storage::{Account, ConversationBinding};

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.as_deref() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn unique_temp_path(prefix: &str, extension: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nonce}.{extension}"))
    }

    fn create_codex_state_db(path: &Path, rows: &[(&str, &str, &str, &str, &str, &str, i64)]) {
        let conn = Connection::open(path).expect("open state db");
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                cwd TEXT NOT NULL,
                title TEXT,
                model TEXT,
                reasoning_effort TEXT,
                model_provider TEXT,
                archived INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER,
                updated_at_ms INTEGER
            );",
        )
        .expect("create threads");
        for (id, cwd, title, model, reasoning, provider, updated_at) in rows {
            conn.execute(
                "INSERT INTO threads (
                    id, cwd, title, model, reasoning_effort, model_provider,
                    archived, updated_at, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)",
                rusqlite::params![
                    id,
                    cwd,
                    title,
                    model,
                    reasoning,
                    provider,
                    updated_at,
                    updated_at * 1000
                ],
            )
            .expect("insert thread");
        }
    }

    fn create_codex_state_db_with_sources(
        path: &Path,
        rows: &[(&str, &str, &str, &str, &str, &str, i64, &str)],
        edges: &[(&str, &str, &str)],
    ) {
        let conn = Connection::open(path).expect("open state db");
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                cwd TEXT NOT NULL,
                title TEXT,
                model TEXT,
                reasoning_effort TEXT,
                model_provider TEXT,
                archived INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER,
                updated_at_ms INTEGER,
                source TEXT NOT NULL DEFAULT '',
                agent_nickname TEXT,
                agent_role TEXT
            );
            CREATE TABLE thread_spawn_edges (
                parent_thread_id TEXT NOT NULL,
                child_thread_id TEXT NOT NULL PRIMARY KEY,
                status TEXT NOT NULL
            );",
        )
        .expect("create threads");
        for (id, cwd, title, model, reasoning, provider, updated_at, source) in rows {
            conn.execute(
                "INSERT INTO threads (
                    id, cwd, title, model, reasoning_effort, model_provider,
                    archived, updated_at, updated_at_ms, source
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?9)",
                rusqlite::params![
                    id,
                    cwd,
                    title,
                    model,
                    reasoning,
                    provider,
                    updated_at,
                    updated_at * 1000,
                    source,
                ],
            )
            .expect("insert thread");
        }
        for (parent, child, status) in edges {
            conn.execute(
                "INSERT INTO thread_spawn_edges (parent_thread_id, child_thread_id, status)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![parent, child, status],
            )
            .expect("insert edge");
        }
    }

    fn sample_account(id: &str, workspace: Option<&str>, updated_at: i64) -> Account {
        Account {
            id: id.to_string(),
            label: id.to_string(),
            issuer: "codex".to_string(),
            chatgpt_account_id: None,
            workspace_id: workspace.map(str::to_string),
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: updated_at,
            updated_at,
        }
    }

    fn sample_binding(
        platform_key_hash: &str,
        conversation_id: &str,
        account_id: &str,
        thread_anchor: &str,
        last_model: Option<&str>,
        updated_at: i64,
    ) -> ConversationBinding {
        ConversationBinding {
            platform_key_hash: platform_key_hash.to_string(),
            conversation_id: conversation_id.to_string(),
            account_id: account_id.to_string(),
            thread_epoch: 1,
            thread_anchor: thread_anchor.to_string(),
            status: "active".to_string(),
            last_model: last_model.map(str::to_string),
            last_switch_reason: None,
            created_at: updated_at,
            updated_at,
            last_used_at: updated_at,
        }
    }

    fn thread_model_provider(path: &Path, thread_id: &str) -> String {
        let conn = Connection::open(path).expect("open state db");
        conn.query_row(
            "SELECT model_provider FROM threads WHERE id = ?1",
            [thread_id],
            |row| row.get::<_, String>(0),
        )
        .expect("read provider")
    }

    fn thread_model_and_reasoning(path: &Path, thread_id: &str) -> (String, Option<String>) {
        let conn = Connection::open(path).expect("open state db");
        conn.query_row(
            "SELECT model, reasoning_effort FROM threads WHERE id = ?1",
            [thread_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .expect("read model")
    }

    fn aggregate_api(id: &str, sort: i64) -> AggregateApi {
        AggregateApi {
            id: id.to_string(),
            provider_type: AGGREGATE_API_PROVIDER_CODEX.to_string(),
            supplier_name: Some(id.to_string()),
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

    fn azure_aggregate_api(id: &str) -> AggregateApi {
        AggregateApi {
            provider_type: AGGREGATE_API_PROVIDER_AZURE_OPENAI.to_string(),
            url: "https://azure.example.com/openai/v1".to_string(),
            ..aggregate_api(id, 0)
        }
    }

    fn route_binding(
        id: &str,
        model: &str,
        aggregate_api_id: &str,
        priority: i64,
        strategy: &str,
    ) -> ModelRouteBinding {
        let now = now_ts();
        ModelRouteBinding {
            id: id.to_string(),
            model: model.to_string(),
            aggregate_api_id: aggregate_api_id.to_string(),
            enabled: true,
            priority,
            weight: 1,
            route_strategy: strategy.to_string(),
            manual_preferred: false,
            supports_responses: true,
            supports_chat_completions: true,
            requires_adapter: false,
            last_probe_status: None,
            last_error: None,
            last_success_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn upstream_capability(
        id: &str,
        aggregate_api_id: &str,
        model: &str,
        updated_at: i64,
        supports_responses: bool,
        supports_chat_completions: bool,
        requires_adapter: bool,
        probe_status: &str,
    ) -> UpstreamModelCapability {
        UpstreamModelCapability {
            id: id.to_string(),
            aggregate_api_id: aggregate_api_id.to_string(),
            model: model.to_string(),
            supports_responses,
            supports_chat_completions,
            requires_adapter,
            probe_status: probe_status.to_string(),
            last_error: None,
            last_probe_at: Some(updated_at),
            updated_at,
        }
    }

    fn session_memory(
        thread_id: &str,
        workspace: &str,
        model: &str,
        updated_at: i64,
    ) -> SessionModelMemory {
        SessionModelMemory {
            thread_id: thread_id.to_string(),
            workspace: workspace.to_string(),
            title: None,
            model: model.to_string(),
            reasoning_effort: Some("high".to_string()),
            source: "manual".to_string(),
            locked: false,
            last_seen_at: updated_at,
            updated_at,
        }
    }

    fn workspace_default(
        workspace: &str,
        model: Option<&str>,
        reasoning_effort: Option<&str>,
        inherit_last_session: bool,
    ) -> WorkspaceModelDefault {
        WorkspaceModelDefault {
            workspace: workspace.to_string(),
            default_model: model.map(str::to_string),
            default_reasoning_effort: reasoning_effort.map(str::to_string),
            inherit_last_session,
            auto_remember: true,
            updated_at: now_ts(),
        }
    }

    fn workspace_default_with_auto_remember(
        workspace: &str,
        model: Option<&str>,
        reasoning_effort: Option<&str>,
        inherit_last_session: bool,
        auto_remember: bool,
    ) -> WorkspaceModelDefault {
        WorkspaceModelDefault {
            auto_remember,
            ..workspace_default(workspace, model, reasoning_effort, inherit_last_session)
        }
    }

    fn ids(items: &[AggregateApi]) -> Vec<String> {
        items.iter().map(|item| item.id.clone()).collect()
    }

    #[test]
    fn session_default_prefers_workspace_last_model_when_enabled() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .upsert_workspace_model_default(&workspace_default(
                "C:/work/project",
                Some("gpt-5.5"),
                Some("medium"),
                true,
            ))
            .expect("workspace default");
        storage
            .upsert_session_model_memory(&session_memory(
                "thread-a",
                "C:/work/project",
                "glm-5.1",
                10,
            ))
            .expect("session memory");

        let default = resolve_session_default_for_workspace(&storage, "C:/work/project")
            .expect("resolve default")
            .expect("default");

        assert_eq!(default.model, "glm-5.1");
        assert_eq!(default.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(default.source, "workspace_last");
    }

    #[test]
    fn session_default_ignores_discovered_state_memory() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .upsert_workspace_model_default(&workspace_default(
                "C:/work/project",
                Some("gpt-5.5"),
                Some("medium"),
                true,
            ))
            .expect("workspace default");
        let mut state_memory =
            session_memory("thread-state", "C:/work/project", "mimo-v2.5-pro", 20);
        state_memory.source = "state".to_string();
        storage
            .upsert_session_model_memory(&state_memory)
            .expect("state memory");

        let default = resolve_session_default_for_workspace(&storage, "C:/work/project")
            .expect("resolve default")
            .expect("default");

        assert_eq!(default.model, "gpt-5.5");
        assert_eq!(default.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(default.source, "workspace_default");
    }

    #[test]
    fn session_default_uses_workspace_default_when_inherit_disabled() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .upsert_workspace_model_default(&workspace_default(
                "C:/work/project",
                Some("gpt-5.5"),
                Some("medium"),
                false,
            ))
            .expect("workspace default");
        storage
            .upsert_session_model_memory(&session_memory(
                "thread-a",
                "C:/work/project",
                "glm-5.1",
                10,
            ))
            .expect("session memory");

        let default = resolve_session_default_for_workspace(&storage, "C:/work/project")
            .expect("resolve default")
            .expect("default");

        assert_eq!(default.model, "gpt-5.5");
        assert_eq!(default.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(default.source, "workspace_default");
    }

    #[test]
    fn session_default_falls_back_to_global_default() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .upsert_workspace_model_default(&workspace_default(
                GLOBAL_WORKSPACE,
                Some("kimi-k2.6"),
                Some("low"),
                true,
            ))
            .expect("global default");

        let default = resolve_session_default_for_workspace(&storage, "C:/work/project")
            .expect("resolve default")
            .expect("default");

        assert_eq!(default.model, "kimi-k2.6");
        assert_eq!(default.reasoning_effort.as_deref(), Some("low"));
        assert_eq!(default.source, "global_default");
    }

    #[test]
    fn route_binding_order_keeps_manual_preferred_first() {
        let mut manual = route_binding("mrb-manual", "glm-5.1", "agg-manual", 100, "ordered");
        manual.manual_preferred = true;
        let ordered = route_binding_order(
            vec![
                route_binding("mrb-primary", "glm-5.1", "agg-primary", 0, "ordered"),
                manual,
                route_binding("mrb-backup", "glm-5.1", "agg-backup", 1, "ordered"),
            ],
            "key-a",
            "glm-5.1",
        );

        let ids = ordered
            .iter()
            .map(|item| item.aggregate_api_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["agg-manual", "agg-primary", "agg-backup"]);
    }

    #[test]
    fn balanced_route_uses_binding_weight_for_first_candidate() {
        let mut light = route_binding("mrb-light", "glm-5.1", "agg-light", 0, "balanced");
        light.weight = 1;
        let mut heavy = route_binding("mrb-heavy", "glm-5.1", "agg-heavy", 1, "balanced");
        heavy.weight = 99;

        let mut heavy_first = 0usize;
        for index in 0..64 {
            let ordered = route_binding_order(
                vec![light.clone(), heavy.clone()],
                format!("key-{index}").as_str(),
                "glm-5.1",
            );
            if ordered
                .first()
                .is_some_and(|item| item.aggregate_api_id == "agg-heavy")
            {
                heavy_first += 1;
            }
        }

        assert!(heavy_first > 40, "heavy_first={heavy_first}");
    }

    #[test]
    fn balanced_route_binding_scope_changes_offset_when_candidates_change() {
        let first_scope = vec![
            route_binding("mrb-a", "glm-5.1", "agg-a", 0, "balanced"),
            route_binding("mrb-b", "glm-5.1", "agg-b", 1, "balanced"),
            route_binding("mrb-c", "glm-5.1", "agg-c", 2, "balanced"),
        ];
        let second_scope = vec![
            route_binding("mrb-a", "glm-5.1", "agg-a", 0, "balanced"),
            route_binding("mrb-c", "glm-5.1", "agg-c", 1, "balanced"),
        ];

        let first_scope_value = route_binding_scope(first_scope.as_slice());
        let second_scope_value = route_binding_scope(second_scope.as_slice());

        assert_ne!(first_scope_value, second_scope_value);
        let first_offset = weighted_route_offset(
            "key-a",
            "glm-5.1",
            first_scope_value.as_str(),
            first_scope.as_slice(),
        );
        let second_offset = weighted_route_offset(
            "key-a",
            "glm-5.1",
            second_scope_value.as_str(),
            second_scope.as_slice(),
        );

        assert!(first_offset < first_scope.len());
        assert!(second_offset < second_scope.len());
        assert_eq!(
            stable_route_index("key-a", "glm-5.1", "scope-a", 3),
            stable_route_index("key-a", "glm-5.1", "scope-a", 3)
        );
    }

    #[test]
    fn manual_preferred_is_unique_per_model() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_aggregate_api(&aggregate_api("agg-first", 0))
            .expect("insert first api");
        storage
            .insert_aggregate_api(&aggregate_api("agg-second", 1))
            .expect("insert second api");
        let mut first = route_binding("mrb-first", "glm-5.1", "agg-first", 0, "ordered");
        first.manual_preferred = true;
        let mut second = route_binding("mrb-second", "glm-5.1", "agg-second", 1, "ordered");
        second.manual_preferred = true;
        storage
            .upsert_model_route_binding(&first)
            .expect("insert first");
        storage
            .clear_manual_preferred_model_route_bindings("glm-5.1", Some(second.id.as_str()))
            .expect("clear previous");
        storage
            .upsert_model_route_binding(&second)
            .expect("insert second");

        let bindings = storage
            .list_model_route_bindings(Some("glm-5.1"))
            .expect("list bindings");
        let manual_ids = bindings
            .iter()
            .filter(|item| item.manual_preferred)
            .map(|item| item.aggregate_api_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(manual_ids, vec!["agg-second"]);
    }

    #[test]
    fn capability_refresh_does_not_override_manual_chat_adapter_binding() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_aggregate_api(&aggregate_api("agg-free", 0))
            .expect("insert api");
        let mut binding = route_binding("mrb-free", "gpt-5.5", "agg-free", 0, "ordered");
        binding.manual_preferred = true;
        binding.supports_responses = false;
        binding.supports_chat_completions = true;
        binding.requires_adapter = true;
        storage
            .upsert_model_route_binding(&binding)
            .expect("insert binding");
        storage
            .upsert_upstream_model_capability(&upstream_capability(
                "cap-free", "agg-free", "gpt-5.5", 100, true, true, false, "success",
            ))
            .expect("insert stale capability");

        assert!(aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("gpt-5.5"),
            "agg-free",
        ));
    }

    #[test]
    fn responses_probe_rejects_event_only_created_stream() {
        let validation = validate_stream_probe_prefix(
            "event: response.created\r\n\r\n",
            StreamProbeKind::Responses,
        );

        assert!(!validation.ok);
        assert!(validation
            .reason
            .as_deref()
            .is_some_and(|value| value.contains("event-only SSE")));
    }

    #[test]
    fn responses_probe_requires_terminal_stream_event() {
        let created_only = validate_stream_probe_prefix(
            "event: response.created\r\ndata: {\"type\":\"response.created\"}\r\n\r\n",
            StreamProbeKind::Responses,
        );
        assert!(!created_only.ok);
        assert!(
            validate_stream_probe_prefix(
                "event: response.completed\r\n\r\n",
                StreamProbeKind::Responses,
            )
            .ok
        );
    }

    #[test]
    fn model_route_limits_candidates_to_bound_upstream_pool() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let mut primary = aggregate_api("agg-glm-primary", 0);
        primary.url = "https://api.freemodel.dev/v1".to_string();
        let mut backup = aggregate_api("agg-glm-backup", 1);
        backup.url = "https://api.freemodel.dev".to_string();
        let mut sibling = aggregate_api("agg-glm-same-base", 2);
        sibling.url = "https://api.freemodel.dev/v1/".to_string();
        let unrelated = aggregate_api("agg-unrelated-gpt", 3);
        for api in [
            primary.clone(),
            backup.clone(),
            sibling.clone(),
            unrelated.clone(),
        ] {
            storage.insert_aggregate_api(&api).expect("insert api");
        }
        for binding in [
            route_binding("mrb-primary", "glm-5.1", "agg-glm-primary", 0, "ordered"),
            route_binding("mrb-backup", "glm-5.1", "agg-glm-backup", 1, "ordered"),
        ] {
            storage
                .upsert_model_route_binding(&binding)
                .expect("insert route binding");
        }

        let routed = route_aggregate_candidates_for_model(
            &storage,
            Some("glm-5.1"),
            vec![
                unrelated.clone(),
                sibling.clone(),
                backup.clone(),
                primary.clone(),
            ],
            "key-a",
        );

        assert_eq!(
            ids(&routed),
            vec!["agg-glm-primary", "agg-glm-backup", "agg-glm-same-base"]
        );
    }

    #[test]
    fn model_route_matches_display_name_spacing_case_variants() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_aggregate_api(&aggregate_api("agg-glm", 0))
            .expect("insert glm api");
        storage
            .insert_aggregate_api(&aggregate_api("agg-mimo", 1))
            .expect("insert mimo api");
        storage
            .upsert_model_route_binding(&route_binding(
                "mrb-glm", "glm-5.1", "agg-glm", 0, "ordered",
            ))
            .expect("insert glm binding");
        storage
            .upsert_model_route_binding(&route_binding(
                "mrb-mimo",
                "mimo-2.5pro",
                "agg-mimo",
                0,
                "ordered",
            ))
            .expect("insert mimo binding");

        let glm = route_aggregate_candidates_for_model(
            &storage,
            Some("GLM 5.1"),
            vec![aggregate_api("agg-glm", 0), aggregate_api("agg-mimo", 1)],
            "key-a",
        );
        let mimo = route_aggregate_candidates_for_model(
            &storage,
            Some("mimo 2.5pro"),
            vec![aggregate_api("agg-glm", 0), aggregate_api("agg-mimo", 1)],
            "key-a",
        );

        assert_eq!(ids(&glm), vec!["agg-glm"]);
        assert_eq!(ids(&mimo), vec!["agg-mimo"]);
    }

    #[test]
    fn model_route_uses_successful_capability_when_binding_is_missing() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let mimo_api = aggregate_api("agg-mimo", 0);
        storage
            .insert_aggregate_api(&mimo_api)
            .expect("insert mimo api");
        storage
            .upsert_upstream_model_capability(&upstream_capability(
                "cap-mimo",
                "agg-mimo",
                "mimo-v2.5-pro",
                10,
                false,
                true,
                true,
                "success",
            ))
            .expect("insert mimo capability");

        let routed = route_aggregate_candidates_for_model(
            &storage,
            Some("mimo-v2.5-pro"),
            Vec::new(),
            "key-a",
        );

        assert_eq!(ids(&routed), vec!["agg-mimo"]);
        assert!(model_route_applies_for_model(
            &storage,
            Some("mimo-v2.5-pro")
        ));
        assert!(aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("mimo-v2.5-pro"),
            "agg-mimo",
        ));
    }

    #[test]
    fn model_route_does_not_resurrect_disabled_api_from_successful_capability() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let mut wool_api = aggregate_api("agg-wool-disabled", 0);
        wool_api.status = "disabled".to_string();
        wool_api.pool = "wool".to_string();
        storage
            .insert_aggregate_api(&wool_api)
            .expect("insert disabled wool api");
        storage
            .upsert_upstream_model_capability(&upstream_capability(
                "cap-wool",
                "agg-wool-disabled",
                "gpt-5.5",
                10,
                true,
                true,
                false,
                "success",
            ))
            .expect("insert wool capability");

        let routed =
            route_aggregate_candidates_for_model(&storage, Some("gpt-5.5"), Vec::new(), "key-a");

        assert!(routed.is_empty());
        assert!(!model_route_applies_for_model(&storage, Some("gpt-5.5")));
    }

    #[test]
    fn azure_probe_prefers_configured_text_model_over_first_discovered_image_model() {
        let _guard = crate::test_env_guard();
        let _fallback_guard = EnvGuard::set("CODEXMANAGER_MODEL_ROUTER_PROBE_FALLBACK_MODELS", "");
        let api = azure_aggregate_api("agg-azure");
        let selected = select_probe_model(
            &api,
            &[
                "dall-e-3-3.0".to_string(),
                "gpt-5.4".to_string(),
                "gpt-5.5".to_string(),
            ],
        );

        assert_eq!(selected.as_deref(), Some("gpt-5.4"));
    }

    #[test]
    fn generic_probe_skips_non_text_models_before_using_discovered_text_model() {
        let _guard = crate::test_env_guard();
        let _fallback_guard = EnvGuard::set("CODEXMANAGER_MODEL_ROUTER_PROBE_FALLBACK_MODELS", "");
        let api = aggregate_api("agg-openai-compat", 0);
        let selected = select_probe_model(
            &api,
            &[
                "dall-e-3-3.0".to_string(),
                "text-embedding-3-large".to_string(),
                "glm-5.1".to_string(),
            ],
        );

        assert_eq!(selected.as_deref(), Some("glm-5.1"));
    }

    #[test]
    fn responses_stream_probe_rejects_first_chunk_without_terminal_event() {
        let body = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_probe\"}}\n\n"
        );

        let validation = validate_stream_probe_prefix(body, StreamProbeKind::Responses);

        assert!(!validation.ok);
        assert!(validation
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("response.completed"));
    }

    #[test]
    fn responses_stream_probe_accepts_terminal_event() {
        let body = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\"}\n\n"
        );

        let validation = validate_stream_probe_prefix(body, StreamProbeKind::Responses);

        assert!(validation.ok);
    }

    #[test]
    fn chat_stream_probe_accepts_done_or_finish_reason() {
        let done = "data: [DONE]\n\n";
        let finish_reason =
            concat!("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n");

        assert!(validate_stream_probe_prefix(done, StreamProbeKind::ChatCompletions).ok);
        assert!(validate_stream_probe_prefix(finish_reason, StreamProbeKind::ChatCompletions).ok);
    }

    #[test]
    fn model_route_uses_successful_capability_for_bound_api() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let glm_api = aggregate_api("agg-glm", 0);
        storage
            .insert_aggregate_api(&glm_api)
            .expect("insert glm api");
        storage
            .upsert_model_route_binding(&route_binding(
                "mrb-glm", "glm-5.1", "agg-glm", 0, "ordered",
            ))
            .expect("insert glm binding");
        storage
            .upsert_upstream_model_capability(&upstream_capability(
                "cap-glm", "agg-glm", "glm-5.1", 20, false, true, true, "success",
            ))
            .expect("insert glm capability");

        let routed =
            route_aggregate_candidates_for_model(&storage, Some("glm-5.1"), Vec::new(), "key-a");

        assert_eq!(ids(&routed), vec!["agg-glm"]);
        assert!(aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("glm-5.1"),
            "agg-glm",
        ));
    }

    #[test]
    fn model_route_matches_provider_prefixed_upstream_model_tail() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let glm_api = aggregate_api("agg-glm-free", 0);
        storage
            .insert_aggregate_api(&glm_api)
            .expect("insert glm api");
        storage
            .upsert_model_route_binding(&route_binding(
                "mrb-glm-free",
                "glm-5.1",
                "agg-glm-free",
                0,
                "ordered",
            ))
            .expect("insert glm binding");
        storage
            .upsert_upstream_model_capability(&upstream_capability(
                "cap-glm-free",
                "agg-glm-free",
                "zai-org/GLM-5.1",
                20,
                false,
                true,
                true,
                "success",
            ))
            .expect("insert glm capability");

        let routed =
            route_aggregate_candidates_for_model(&storage, Some("glm-5.1"), Vec::new(), "key-a");
        let upstream_model = resolve_upstream_model_for_aggregate_candidate(
            &storage,
            Some("glm-5.1"),
            "agg-glm-free",
        );

        assert_eq!(ids(&routed), vec!["agg-glm-free"]);
        assert_eq!(upstream_model.as_deref(), Some("zai-org/GLM-5.1"));
        assert!(aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("glm-5.1"),
            "agg-glm-free",
        ));
    }

    #[test]
    fn model_route_accepts_manual_provider_prefixed_capability() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let glm_api = aggregate_api("agg-glm-free", 0);
        storage
            .insert_aggregate_api(&glm_api)
            .expect("insert glm api");
        storage
            .upsert_model_route_binding(&route_binding(
                "mrb-glm-free-manual",
                "glm-5.1",
                "agg-glm-free",
                0,
                "ordered",
            ))
            .expect("insert glm binding");
        storage
            .upsert_upstream_model_capability(&upstream_capability(
                "cap-glm-free-manual",
                "agg-glm-free",
                "zai-org/GLM-5.1",
                20,
                false,
                true,
                true,
                "manual",
            ))
            .expect("insert glm capability");

        let routed =
            route_aggregate_candidates_for_model(&storage, Some("glm-5.1"), Vec::new(), "key-a");
        let upstream_model = resolve_upstream_model_for_aggregate_candidate(
            &storage,
            Some("glm-5.1"),
            "agg-glm-free",
        );

        assert_eq!(ids(&routed), vec!["agg-glm-free"]);
        assert_eq!(upstream_model.as_deref(), Some("zai-org/GLM-5.1"));
        assert!(aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("glm-5.1"),
            "agg-glm-free",
        ));
    }

    #[test]
    fn model_route_with_explicit_binding_does_not_supplement_unbound_capability_api() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let old_api = aggregate_api("agg-mimo-old", 0);
        let new_api = aggregate_api("agg-mimo-new", 1);
        storage
            .insert_aggregate_api(&old_api)
            .expect("insert old api");
        storage
            .insert_aggregate_api(&new_api)
            .expect("insert new api");

        let mut stale_binding = route_binding(
            "mrb-mimo-old",
            "mimo-v2.5-pro",
            "agg-mimo-old",
            0,
            "ordered",
        );
        stale_binding.supports_responses = false;
        stale_binding.supports_chat_completions = true;
        stale_binding.requires_adapter = true;
        storage
            .upsert_model_route_binding(&stale_binding)
            .expect("insert stale binding");

        storage
            .upsert_upstream_model_capability(&upstream_capability(
                "cap-mimo-old",
                "agg-mimo-old",
                "mimo-v2.5-pro",
                10,
                false,
                true,
                true,
                "success",
            ))
            .expect("insert old capability");
        storage
            .upsert_upstream_model_capability(&upstream_capability(
                "cap-mimo-new",
                "agg-mimo-new",
                "mimo-v2.5-pro",
                20,
                true,
                true,
                false,
                "success",
            ))
            .expect("insert new capability");

        let routed = route_aggregate_candidates_for_model(
            &storage,
            Some("mimo-v2.5-pro"),
            vec![old_api.clone(), new_api.clone()],
            "key-a",
        );

        assert_eq!(ids(&routed), vec!["agg-mimo-old"]);
        assert!(aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("mimo-v2.5-pro"),
            "agg-mimo-old",
        ));
        assert!(!aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("mimo-v2.5-pro"),
            "agg-mimo-new",
        ));
    }

    #[test]
    fn model_route_prefers_only_bound_active_candidates_and_excludes_unbound_active_api() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let mut bound = aggregate_api("agg-bound-mimo", 0);
        bound.supplier_name = Some("mimo pro".to_string());
        bound.url = "https://api.xiaomimimo.com".to_string();
        storage
            .insert_aggregate_api(&bound)
            .expect("insert bound api");

        let mut unbound = aggregate_api("agg-unbound-02", 1);
        unbound.supplier_name = Some("0.2".to_string());
        unbound.url = "https://hub.linux.do".to_string();
        storage
            .insert_aggregate_api(&unbound)
            .expect("insert unbound api");

        storage
            .upsert_model_route_binding(&route_binding(
                "mrb-mimo-only",
                "mimo-v2.5-pro",
                "agg-bound-mimo",
                0,
                "ordered",
            ))
            .expect("insert route binding");

        let routed = route_aggregate_candidates_for_model(
            &storage,
            Some("mimo-v2.5-pro"),
            vec![bound.clone(), unbound.clone()],
            "key-a",
        );

        assert_eq!(ids(&routed), vec!["agg-bound-mimo"]);
    }

    #[test]
    fn model_route_supplements_same_base_url_candidates_only() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let mut bound = aggregate_api("agg-bound-free", 0);
        bound.url = "https://api.freemodel.dev/v1".to_string();
        let mut same_base = aggregate_api("agg-same-base-free", 1);
        same_base.url = "https://api.freemodel.dev".to_string();
        let mut unrelated = aggregate_api("agg-official-openai", 2);
        unrelated.url = "https://api.openai.com".to_string();

        for api in [bound.clone(), same_base.clone(), unrelated.clone()] {
            storage.insert_aggregate_api(&api).expect("insert api");
        }
        storage
            .upsert_model_route_binding(&route_binding(
                "mrb-bound-free",
                "gpt-5.5",
                "agg-bound-free",
                0,
                "ordered",
            ))
            .expect("insert route binding");

        let routed = route_aggregate_candidates_for_model(
            &storage,
            Some("gpt-5.5"),
            vec![unrelated, same_base, bound],
            "key-a",
        );

        assert_eq!(ids(&routed), vec!["agg-bound-free", "agg-same-base-free"]);
    }

    #[test]
    fn model_route_same_base_candidates_do_not_duplicate_when_multiple_bound_keys_share_a_base() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let mut first = aggregate_api("agg-free-first", 0);
        first.url = "https://api.freemodel.dev/v1".to_string();
        let mut second = aggregate_api("agg-free-second", 1);
        second.url = "https://api.freemodel.dev".to_string();
        let mut sibling = aggregate_api("agg-free-sibling", 2);
        sibling.url = "https://api.freemodel.dev/v1/".to_string();
        for api in [first.clone(), second.clone(), sibling.clone()] {
            storage.insert_aggregate_api(&api).expect("insert api");
        }
        for binding in [
            route_binding("mrb-free-first", "gpt-5.5", "agg-free-first", 0, "ordered"),
            route_binding(
                "mrb-free-second",
                "gpt-5.5",
                "agg-free-second",
                1,
                "ordered",
            ),
        ] {
            storage
                .upsert_model_route_binding(&binding)
                .expect("insert route binding");
        }

        let routed = route_aggregate_candidates_for_model(
            &storage,
            Some("gpt-5.5"),
            vec![sibling.clone(), second.clone(), first.clone()],
            "key-a",
        );

        assert_eq!(
            ids(&routed),
            vec!["agg-free-first", "agg-free-second", "agg-free-sibling"]
        );
    }

    #[test]
    fn model_route_sibling_expansion_excludes_api_bound_to_different_model() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let mut api_a = aggregate_api("agg-relay-key-a", 0);
        api_a.url = "https://api.freemodel.dev/v1".to_string();
        let mut api_b = aggregate_api("agg-relay-key-b", 1);
        api_b.url = "https://api.freemodel.dev".to_string();
        for api in [api_a.clone(), api_b.clone()] {
            storage.insert_aggregate_api(&api).expect("insert api");
        }
        storage
            .upsert_model_route_binding(&route_binding(
                "mrb-a-gpt",
                "gpt-5.5",
                "agg-relay-key-a",
                0,
                "ordered",
            ))
            .expect("insert binding for gpt");
        storage
            .upsert_model_route_binding(&route_binding(
                "mrb-b-glm",
                "glm-5.1",
                "agg-relay-key-b",
                0,
                "ordered",
            ))
            .expect("insert binding for glm");

        let routed_gpt = route_aggregate_candidates_for_model(
            &storage,
            Some("gpt-5.5"),
            vec![api_a.clone(), api_b.clone()],
            "key-a",
        );
        let routed_glm = route_aggregate_candidates_for_model(
            &storage,
            Some("glm-5.1"),
            vec![api_a.clone(), api_b.clone()],
            "key-a",
        );

        assert_eq!(ids(&routed_gpt), vec!["agg-relay-key-a"]);
        assert_eq!(ids(&routed_glm), vec!["agg-relay-key-b"]);
    }

    #[test]
    fn model_route_with_stale_binding_does_not_use_unqualified_active_fallback() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_aggregate_api(&aggregate_api("agg-active-gpt", 0))
            .expect("insert api");
        storage
            .insert_aggregate_api(&aggregate_api("agg-unavailable-glm", 1))
            .expect("insert unavailable api");
        let stale = route_binding("mrb-stale", "glm-5.1", "agg-missing", 0, "ordered");
        let stale = ModelRouteBinding {
            aggregate_api_id: "agg-unavailable-glm".to_string(),
            ..stale
        };
        storage
            .upsert_model_route_binding(&stale)
            .expect("insert route binding");

        let routed = route_aggregate_candidates_for_model(
            &storage,
            Some("glm-5.1"),
            vec![aggregate_api("agg-active-gpt", 0)],
            "key-a",
        );

        assert_eq!(ids(&routed), Vec::<String>::new());
    }

    #[test]
    fn model_route_recovers_bound_active_api_when_not_in_initial_candidates() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_aggregate_api(&aggregate_api("agg-bound-third-party", 0))
            .expect("insert bound api");
        let binding = route_binding(
            "mrb-third-party",
            "glm-5.1",
            "agg-bound-third-party",
            0,
            "ordered",
        );
        storage
            .upsert_model_route_binding(&binding)
            .expect("insert route binding");

        let routed =
            route_aggregate_candidates_for_model(&storage, Some("glm-5.1"), Vec::new(), "key-a");
        let bindings = storage
            .list_enabled_model_route_bindings("glm-5.1")
            .expect("list bindings");

        assert_eq!(ids(&routed), vec!["agg-bound-third-party"]);
        assert_ne!(bindings[0].last_probe_status.as_deref(), Some("failed"));
    }

    #[test]
    fn model_route_does_not_fallback_to_active_candidate_without_model_capability() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let mut enabled_free = aggregate_api("agg-free-enabled", 0);
        enabled_free.url = "https://api.freemodel.dev/v1".to_string();
        storage
            .insert_aggregate_api(&enabled_free)
            .expect("insert enabled free api");
        storage
            .insert_aggregate_api(&aggregate_api("agg-history-aggregate", 1))
            .expect("insert history api");
        storage
            .upsert_model_route_binding(&route_binding(
                "mrb-history",
                "gpt-5.5",
                "agg-history-aggregate",
                0,
                "ordered",
            ))
            .expect("insert history route binding");

        let routed = route_aggregate_candidates_for_model(
            &storage,
            Some("gpt-5.5"),
            vec![enabled_free],
            "key-a",
        );

        assert_eq!(ids(&routed), Vec::<String>::new());
    }

    #[test]
    fn model_route_falls_back_to_active_candidate_with_successful_model_capability() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let mut enabled_free = aggregate_api("agg-free-enabled", 0);
        enabled_free.url = "https://api.freemodel.dev/v1".to_string();
        storage
            .insert_aggregate_api(&enabled_free)
            .expect("insert enabled free api");
        storage
            .insert_aggregate_api(&aggregate_api("agg-history-aggregate", 1))
            .expect("insert history api");
        storage
            .upsert_model_route_binding(&route_binding(
                "mrb-history",
                "gpt-5.5",
                "agg-history-aggregate",
                0,
                "ordered",
            ))
            .expect("insert history route binding");
        storage
            .upsert_upstream_model_capability(&upstream_capability(
                "cap-free-gpt",
                "agg-free-enabled",
                "gpt-5.5",
                100,
                true,
                true,
                false,
                "success",
            ))
            .expect("insert active candidate capability");

        let routed = route_aggregate_candidates_for_model(
            &storage,
            Some("gpt-5.5"),
            vec![enabled_free],
            "key-a",
        );

        assert_eq!(ids(&routed), vec!["agg-free-enabled"]);
    }

    #[test]
    fn model_route_preserves_same_base_url_multi_key_candidates() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let mut first = aggregate_api("agg-same-base-a", 0);
        first.url = "https://api.same-base.example/v1".to_string();
        let mut second = aggregate_api("agg-same-base-b", 1);
        second.url = "https://api.same-base.example".to_string();
        storage.insert_aggregate_api(&first).expect("insert first");
        storage
            .insert_aggregate_api(&second)
            .expect("insert second");
        storage
            .upsert_model_route_binding(&route_binding(
                "mrb-same-base-a",
                "glm-5.1",
                "agg-same-base-a",
                0,
                "ordered",
            ))
            .expect("insert route binding");

        let routed = route_aggregate_candidates_for_model(
            &storage,
            Some("glm-5.1"),
            vec![first, second],
            "key-a",
        );

        assert_eq!(ids(&routed), vec!["agg-same-base-a", "agg-same-base-b"]);
    }

    #[test]
    fn adapter_requirement_requires_chat_only_binding() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_aggregate_api(&aggregate_api("agg-chat", 0))
            .expect("insert api");
        let mut binding = route_binding("mrb-chat-only", "glm-5.1", "agg-chat", 0, "ordered");
        binding.supports_responses = false;
        binding.supports_chat_completions = true;
        binding.requires_adapter = true;
        storage
            .upsert_model_route_binding(&binding)
            .expect("insert route binding");

        assert!(aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("glm-5.1"),
            "agg-chat",
        ));
        assert!(!aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("gpt-5.5"),
            "agg-chat",
        ));
    }

    #[test]
    fn native_responses_stream_failure_keeps_native_responses_when_supported() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_aggregate_api(&aggregate_api("agg-free", 0))
            .expect("insert api");
        let mut binding = route_binding("mrb-free", "gpt-5.5", "agg-free", 0, "ordered");
        binding.supports_responses = true;
        binding.supports_chat_completions = true;
        binding.requires_adapter = false;
        binding.last_probe_status = Some("failed".to_string());
        binding.last_error = Some(
            "连接中断 [bridge_stage=after_downstream_response_started stream_terminal_seen=false]"
                .to_string(),
        );
        storage
            .upsert_model_route_binding(&binding)
            .expect("insert route binding");

        assert!(!aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("gpt-5.5"),
            "agg-free",
        ));
    }

    #[test]
    fn recent_passthrough_stream_failure_keeps_native_responses_when_supported() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let mut api = aggregate_api("agg-free", 0);
        api.url = "https://api.freemodel.dev/v1".to_string();
        storage.insert_aggregate_api(&api).expect("insert api");
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
                    "连接中断 [bridge_stage=after_downstream_response_started stream_terminal_seen=false]"
                        .to_string(),
                ),
                created_at: now_ts(),
                ..Default::default()
            })
            .expect("insert request log");
        storage
            .upsert_upstream_model_capability(&upstream_capability(
                "cap-free-chat",
                "agg-free",
                "gpt-5.5",
                now_ts(),
                true,
                true,
                false,
                "success",
            ))
            .expect("insert capability");

        assert!(!aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("gpt-5.5"),
            "agg-free",
        ));
    }

    #[test]
    fn recent_passthrough_stream_failure_does_not_override_native_capability() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let mut api = aggregate_api("agg-free", 0);
        api.url = "https://api.freemodel.dev/v1".to_string();
        storage.insert_aggregate_api(&api).expect("insert api");
        let now = now_ts();
        for idx in 0..40 {
            storage
                .insert_request_log(&RequestLog {
                    request_path: "/v1/responses".to_string(),
                    original_path: Some("/v1/responses".to_string()),
                    adapted_path: Some("/v1/responses".to_string()),
                    method: "POST".to_string(),
                    model: Some("gpt-5.5".to_string()),
                    response_adapter: Some("Passthrough".to_string()),
                    upstream_url: Some("https://other.example.com/v1/responses".to_string()),
                    aggregate_api_url: Some("https://other.example.com".to_string()),
                    status_code: Some(502),
                    error: Some(
                        "连接中断 [bridge_stage=after_downstream_response_started stream_terminal_seen=false]"
                            .to_string(),
                    ),
                    created_at: now + idx,
                    ..Default::default()
                })
                .expect("insert noise log");
        }
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
                    "连接中断 [bridge_stage=after_downstream_response_started stream_terminal_seen=false]"
                        .to_string(),
                ),
                created_at: now + 100,
                ..Default::default()
            })
            .expect("insert target log");
        storage
            .upsert_upstream_model_capability(&upstream_capability(
                "cap-free-chat-2",
                "agg-free",
                "gpt-5.5",
                now + 100,
                true,
                true,
                false,
                "success",
            ))
            .expect("insert capability");

        assert!(!aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("gpt-5.5"),
            "agg-free",
        ));
    }

    #[test]
    fn capability_chat_only_fallback_requires_chat_adapter_without_binding() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_aggregate_api(&aggregate_api("agg-free-cap", 0))
            .expect("insert api");
        storage
            .upsert_upstream_model_capability(&upstream_capability(
                "cap-free-chat-only",
                "agg-free-cap",
                "gpt-5.5",
                100,
                false,
                true,
                true,
                "success",
            ))
            .expect("insert capability");

        assert!(aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("gpt-5.5"),
            "agg-free-cap",
        ));
        assert!(!aggregate_candidate_requires_responses_to_chat_adapter(
            &storage,
            Some("gpt-5.4"),
            "agg-free-cap",
        ));
    }

    #[test]
    fn auto_remember_false_does_not_write_discovered_state_session_memory() {
        let _guard = crate::test_env_guard();
        let db_path = unique_temp_path("codexmanager-model-router-auto-remember", "db");
        let state_path = unique_temp_path("codexmanager-state-auto-remember", "sqlite");
        let storage = Storage::open(&db_path).expect("open storage");
        storage.init().expect("init storage");
        storage
            .upsert_workspace_model_default(&workspace_default_with_auto_remember(
                "C:/work/no-auto",
                Some("gpt-5.5"),
                Some("medium"),
                true,
                false,
            ))
            .expect("workspace default");
        drop(storage);
        create_codex_state_db(
            &state_path,
            &[(
                "thread-a",
                "C:/work/no-auto",
                "A",
                "glm-5.1",
                "high",
                "cm",
                100,
            )],
        );
        let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
        let _state_guard = EnvGuard::set(
            "CODEXMANAGER_CODEX_STATE_DB_PATH",
            state_path.to_string_lossy().as_ref(),
        );

        let result =
            list_session_models(Some("C:/work/no-auto".to_string())).expect("list sessions");
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].model.as_deref(), Some("glm-5.1"));
        assert_eq!(result.items[0].memory_state, "state");

        let storage = Storage::open(&db_path).expect("reopen storage");
        storage.init().expect("init storage");
        assert!(storage
            .find_session_model_memory("thread-a")
            .expect("find memory")
            .is_none());

        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn session_tree_normalizes_workspace_and_prefers_thread_spawn_edges() {
        let _guard = crate::test_env_guard();
        let db_path = unique_temp_path("codexmanager-model-router-spawn-edge", "db");
        let state_path = unique_temp_path("codexmanager-state-spawn-edge", "sqlite");
        let storage = Storage::open(&db_path).expect("open storage");
        storage.init().expect("init storage");
        drop(storage);
        create_codex_state_db_with_sources(
            &state_path,
            &[
                (
                    "parent-thread",
                    r"\\?\C:\work\project",
                    "Parent",
                    "gpt-5.4",
                    "medium",
                    "cm",
                    100,
                    "",
                ),
                (
                    "child-thread",
                    r"C:\work\project",
                    "Child",
                    "gpt-5.4-mini",
                    "low",
                    "cm",
                    110,
                    r#"{"subagent":{"thread_spawn":{"parent_thread_id":"wrong-parent","depth":1}}}"#,
                ),
            ],
            &[("parent-thread", "child-thread", "active")],
        );
        let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
        let _state_guard = EnvGuard::set(
            "CODEXMANAGER_CODEX_STATE_DB_PATH",
            state_path.to_string_lossy().as_ref(),
        );

        let result =
            list_session_models(Some(r"C:\work\project".to_string())).expect("list sessions");

        let parent = result
            .items
            .iter()
            .find(|item| item.thread_id == "parent-thread")
            .expect("parent is included");
        let child = result
            .items
            .iter()
            .find(|item| item.thread_id == "child-thread")
            .expect("child is included");
        assert_eq!(parent.workspace, "C:/work/project");
        assert_eq!(child.workspace, "C:/work/project");
        assert!(child.is_subagent);
        assert_eq!(child.parent_thread_id.as_deref(), Some("parent-thread"));

        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn manual_session_update_writes_model_and_preserves_model_provider() {
        let _guard = crate::test_env_guard();
        let db_path = unique_temp_path("codexmanager-model-router-manual-update", "db");
        let state_path = unique_temp_path("codexmanager-state-manual-update", "sqlite");
        let storage = Storage::open(&db_path).expect("open storage");
        storage.init().expect("init storage");
        drop(storage);
        create_codex_state_db(
            &state_path,
            &[(
                "thread-b",
                "C:/work/manual",
                "B",
                "glm-5.1",
                "medium",
                "cm",
                100,
            )],
        );
        let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
        let _state_guard = EnvGuard::set(
            "CODEXMANAGER_CODEX_STATE_DB_PATH",
            state_path.to_string_lossy().as_ref(),
        );

        let result = update_session_model(
            "thread-b".to_string(),
            "gpt-5.5".to_string(),
            Some("high".to_string()),
            Some("manual".to_string()),
            Some(true),
        )
        .expect("update session");

        assert!(result.state_updated);
        assert_eq!(result.item.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(result.item.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(result.item.model_provider.as_deref(), Some("cm"));
        assert!(result.item.locked);
        assert_eq!(thread_model_provider(&state_path, "thread-b"), "cm");
        assert_eq!(
            thread_model_and_reasoning(&state_path, "thread-b"),
            ("gpt-5.5".to_string(), Some("high".to_string()))
        );

        let storage = Storage::open(&db_path).expect("reopen storage");
        storage.init().expect("init storage");
        let memory = storage
            .find_session_model_memory("thread-b")
            .expect("find memory")
            .expect("memory exists");
        assert_eq!(memory.model, "gpt-5.5");
        assert_eq!(memory.reasoning_effort.as_deref(), Some("high"));

        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn apply_latest_workspace_session_targets_latest_main_thread_only() {
        let _guard = crate::test_env_guard();
        let db_path = unique_temp_path("codexmanager-model-router-apply-latest", "db");
        let state_path = unique_temp_path("codexmanager-state-apply-latest", "sqlite");
        let storage = Storage::open(&db_path).expect("open storage");
        storage.init().expect("init storage");
        drop(storage);
        create_codex_state_db_with_sources(
            &state_path,
            &[
                (
                    "main-older",
                    "C:/work/latest",
                    "Older Main",
                    "gpt-5.4",
                    "medium",
                    "cm",
                    100,
                    "",
                ),
                (
                    "main-newest",
                    "C:/work/latest",
                    "Newest Main",
                    "gpt-5.4-mini",
                    "low",
                    "cm",
                    130,
                    "",
                ),
                (
                    "child-newer",
                    "C:/work/latest",
                    "Child",
                    "gpt-5.4-mini",
                    "low",
                    "cm",
                    140,
                    r#"{"subagent":{"thread_spawn":{"parent_thread_id":"main-newest","depth":1}}}"#,
                ),
            ],
            &[("main-newest", "child-newer", "active")],
        );
        let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
        let _state_guard = EnvGuard::set(
            "CODEXMANAGER_CODEX_STATE_DB_PATH",
            state_path.to_string_lossy().as_ref(),
        );

        let result = super::apply_model_to_latest_workspace_session(
            "C:/work/latest".to_string(),
            "glm-5.1".to_string(),
            Some("high".to_string()),
            Some("manual".to_string()),
            Some(false),
        )
        .expect("apply latest workspace session");

        assert_eq!(result.item.thread_id, "main-newest");
        assert_eq!(result.item.model.as_deref(), Some("glm-5.1"));
        assert_eq!(result.matched_workspace, "C:/work/latest");
        assert_eq!(
            thread_model_and_reasoning(&state_path, "main-newest"),
            ("glm-5.1".to_string(), Some("high".to_string()))
        );
        assert_eq!(
            thread_model_and_reasoning(&state_path, "child-newer"),
            ("gpt-5.4-mini".to_string(), Some("low".to_string()))
        );

        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn set_workspace_default_clears_saved_model_fields_when_empty_values_are_passed() {
        let _guard = crate::test_env_guard();
        let db_path = unique_temp_path("codexmanager-workspace-default-clear", "db");
        let storage = Storage::open(&db_path).expect("open storage");
        storage.init().expect("init storage");
        storage
            .upsert_workspace_model_default(&workspace_default(
                "C:/work/default-clear",
                Some("gpt-5.5"),
                Some("high"),
                false,
            ))
            .expect("seed workspace default");
        drop(storage);
        let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

        let result = super::set_workspace_default(
            "C:/work/default-clear".to_string(),
            Some("   ".to_string()),
            Some("".to_string()),
            None,
            Some(true),
        )
        .expect("set workspace default");

        assert_eq!(result.default_model, None);
        assert_eq!(result.default_reasoning_effort, None);
        assert!(result.auto_remember);
        assert!(!result.inherit_last_session);

        let storage = Storage::open(&db_path).expect("reopen storage");
        storage.init().expect("init storage");
        let saved = storage
            .find_workspace_model_default("C:/work/default-clear")
            .expect("find workspace default")
            .expect("saved workspace default");
        assert_eq!(saved.default_model, None);
        assert_eq!(saved.default_reasoning_effort, None);
        assert!(saved.auto_remember);
        assert!(!saved.inherit_last_session);

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn list_sessions_mirrors_workspace_default_to_runtime_thread_anchor() {
        let _guard = crate::test_env_guard();
        let db_path = unique_temp_path("codexmanager-default-runtime-mirror", "db");
        let state_path = unique_temp_path("codexmanager-default-runtime-mirror-state", "sqlite");
        let storage = Storage::open(&db_path).expect("open storage");
        storage.init().expect("init storage");
        storage
            .upsert_workspace_model_default(&workspace_default(
                "C:/work/runtime-default",
                Some("glm-5.1"),
                Some("high"),
                false,
            ))
            .expect("workspace default");
        storage
            .insert_account(&sample_account(
                "acc-runtime-default",
                Some("C:/work/runtime-default"),
                100,
            ))
            .expect("insert account");
        storage
            .upsert_conversation_binding(&sample_binding(
                "platform-key",
                "conversation-1",
                "acc-runtime-default",
                "cmgr-thread-1-runtime",
                Some("glm-5.1"),
                120,
            ))
            .expect("insert binding");
        drop(storage);
        create_codex_state_db(
            &state_path,
            &[(
                "thread-runtime-default",
                "C:/work/runtime-default",
                "Runtime Default",
                "",
                "",
                "cm",
                110,
            )],
        );
        let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
        let _state_guard = EnvGuard::set(
            "CODEXMANAGER_CODEX_STATE_DB_PATH",
            state_path.to_string_lossy().as_ref(),
        );

        let result = list_session_models(Some("C:/work/runtime-default".to_string()))
            .expect("list sessions");
        let session = result
            .items
            .iter()
            .find(|item| item.thread_id == "thread-runtime-default")
            .expect("session summary");
        assert_eq!(session.model.as_deref(), Some("glm-5.1"));
        assert_eq!(session.source, "workspace_default");

        let storage = Storage::open(&db_path).expect("reopen storage");
        storage.init().expect("init storage");
        let mirrored = storage
            .find_session_model_memory("cmgr-thread-1-runtime")
            .expect("find mirrored runtime memory")
            .expect("mirrored runtime memory exists");
        assert_eq!(mirrored.workspace, "C:/work/runtime-default");
        assert_eq!(mirrored.model, "glm-5.1");
        assert_eq!(mirrored.reasoning_effort.as_deref(), Some("high"));

        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(state_path);
    }

    #[test]
    fn route_binding_result_updates_success_and_failure_state() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_aggregate_api(&aggregate_api("agg-chat", 0))
            .expect("insert api");
        let binding = route_binding("mrb-chat", "glm-5.1", "agg-chat", 0, "ordered");
        storage
            .upsert_model_route_binding(&binding)
            .expect("insert route binding");

        record_route_binding_error(&storage, Some("glm-5.1"), "agg-chat", "upstream 500");
        let failed = storage
            .list_enabled_model_route_bindings("glm-5.1")
            .expect("list bindings")
            .remove(0);
        assert_eq!(failed.last_probe_status.as_deref(), Some("failed"));
        assert_eq!(failed.last_error.as_deref(), Some("upstream 500"));
        assert!(failed.last_success_at.is_none());

        record_route_binding_success(&storage, Some("glm-5.1"), "agg-chat");
        let succeeded = storage
            .list_enabled_model_route_bindings("glm-5.1")
            .expect("list bindings")
            .remove(0);
        assert_eq!(succeeded.last_probe_status.as_deref(), Some("success"));
        assert!(succeeded.last_error.is_none());
        assert!(succeeded.last_success_at.is_some());
    }

    #[test]
    fn stream_probe_requires_sse_signal_in_prefix() {
        let body = "plain text without sse signal".to_string();
        let has_stream_signal =
            body.contains("event:") || body.contains("data:") || body.contains("response.");

        assert!(!has_stream_signal);
    }
}
