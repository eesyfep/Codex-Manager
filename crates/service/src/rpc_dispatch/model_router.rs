use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "modelRouter/session/list" => {
            let workspace = super::string_param(req, "workspace");
            super::value_or_error(crate::model_router_list_sessions(workspace))
        }
        "modelRouter/session/updateModel" => {
            let thread_id = super::string_param(req, "threadId").unwrap_or_default();
            let model = super::string_param(req, "model").unwrap_or_default();
            let reasoning_effort = super::string_param(req, "reasoningEffort");
            let source = super::string_param(req, "source");
            let locked = super::bool_param(req, "locked");
            super::value_or_error(crate::model_router_update_session_model(
                thread_id,
                model,
                reasoning_effort,
                source,
                locked,
            ))
        }
        "modelRouter/session/applyLatestForWorkspace" => {
            let workspace = super::string_param(req, "workspace").unwrap_or_default();
            let model = super::string_param(req, "model").unwrap_or_default();
            let reasoning_effort = super::string_param(req, "reasoningEffort");
            let source = super::string_param(req, "source");
            let locked = super::bool_param(req, "locked");
            super::value_or_error(crate::model_router_apply_model_to_latest_workspace_session(
                workspace,
                model,
                reasoning_effort,
                source,
                locked,
            ))
        }
        "modelRouter/session/subagentModel/set" => {
            let parent_thread_id = super::string_param(req, "parentThreadId").unwrap_or_default();
            let model = super::string_param(req, "model").unwrap_or_default();
            let reasoning_effort = super::string_param(req, "reasoningEffort");
            let source = super::string_param(req, "source");
            super::value_or_error(crate::model_router_update_session_subagent_model(
                parent_thread_id,
                model,
                reasoning_effort,
                source,
            ))
        }
        "modelRouter/session/subagentModel/clear" => {
            let parent_thread_id = super::string_param(req, "parentThreadId").unwrap_or_default();
            super::value_or_error(crate::model_router_clear_session_subagent_model(
                parent_thread_id,
            ))
        }
        "modelRouter/workspaceDefault/set" => {
            let workspace = super::string_param(req, "workspace").unwrap_or_default();
            let default_model = super::string_param(req, "defaultModel");
            let default_reasoning_effort = super::string_param(req, "defaultReasoningEffort");
            let inherit_last_session = super::bool_param(req, "inheritLastSession");
            let auto_remember = super::bool_param(req, "autoRemember");
            super::value_or_error(crate::model_router_set_workspace_default(
                workspace,
                default_model,
                default_reasoning_effort,
                inherit_last_session,
                auto_remember,
            ))
        }
        "modelRouter/workspaceDefault/delete" => {
            let workspace = super::string_param(req, "workspace").unwrap_or_default();
            super::value_or_error(crate::model_router_delete_workspace_default(workspace))
        }
        "modelRouter/binding/list" => {
            let model = super::string_param(req, "model");
            super::value_or_error(crate::model_router_list_bindings(model))
        }
        "modelRouter/binding/save" => {
            let id = super::string_param(req, "id");
            let model = super::string_param(req, "model").unwrap_or_default();
            let aggregate_api_id = super::string_param(req, "aggregateApiId").unwrap_or_default();
            let enabled = super::bool_param(req, "enabled");
            let priority = super::i64_param(req, "priority");
            let weight = super::i64_param(req, "weight");
            let route_strategy = super::string_param(req, "routeStrategy");
            let manual_preferred = super::bool_param(req, "manualPreferred");
            let supports_responses = super::bool_param(req, "supportsResponses");
            let supports_chat_completions = super::bool_param(req, "supportsChatCompletions");
            let requires_adapter = super::bool_param(req, "requiresAdapter");
            super::value_or_error(crate::model_router_save_binding(
                id,
                model,
                aggregate_api_id,
                enabled,
                priority,
                weight,
                route_strategy,
                manual_preferred,
                supports_responses,
                supports_chat_completions,
                requires_adapter,
            ))
        }
        "modelRouter/binding/delete" => {
            let id = super::string_param(req, "id").unwrap_or_default();
            super::ok_or_error(crate::model_router_delete_binding(id))
        }
        "modelRouter/probe/run" => {
            let aggregate_api_id = super::string_param(req, "aggregateApiId")
                .or_else(|| super::string_param(req, "id"))
                .unwrap_or_default();
            super::value_or_error(crate::model_router_probe_aggregate_api(aggregate_api_id))
        }
        "modelRouter/probe/runAll" => {
            super::value_or_error(crate::model_router_probe_all_aggregate_api())
        }
        "modelRouter/probe/manualModel" => {
            let aggregate_api_id = super::string_param(req, "aggregateApiId")
                .or_else(|| super::string_param(req, "id"))
                .unwrap_or_default();
            let model = super::string_param(req, "model").unwrap_or_default();
            let supports_responses = super::bool_param(req, "supportsResponses");
            let supports_chat_completions = super::bool_param(req, "supportsChatCompletions");
            let requires_adapter = super::bool_param(req, "requiresAdapter");
            super::value_or_error(crate::model_router_add_manual_probe_model(
                aggregate_api_id,
                model,
                supports_responses,
                supports_chat_completions,
                requires_adapter,
            ))
        }
        "modelRouter/probe/quickCall" => {
            let aggregate_api_id = super::string_param(req, "aggregateApiId")
                .or_else(|| super::string_param(req, "id"))
                .unwrap_or_default();
            let model = super::string_param(req, "model").unwrap_or_default();
            super::value_or_error(crate::model_router_quick_check(aggregate_api_id, model))
        }
        "modelRouter/probe/apply" => {
            let probe_run_id = super::string_param(req, "probeRunId")
                .or_else(|| super::string_param(req, "id"))
                .unwrap_or_default();
            super::value_or_error(crate::model_router_apply_probe_candidates(probe_run_id))
        }
        "modelRouter/probe/applySelected" => {
            let probe_run_id = super::string_param(req, "probeRunId")
                .or_else(|| super::string_param(req, "id"))
                .unwrap_or_default();
            let candidate_ids = req
                .params
                .as_ref()
                .and_then(|value| value.get("candidateIds"))
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            super::value_or_error(crate::model_router_apply_selected_probe_candidates(
                probe_run_id,
                candidate_ids,
            ))
        }
        "modelRouter/probe/list" => {
            let limit = super::i64_param(req, "limit").unwrap_or(20);
            super::value_or_error(crate::model_router_list_probe_runs(limit))
        }
        "modelRouter/import/codexManager" => {
            let source_path = super::string_param(req, "sourcePath");
            super::value_or_error(
                crate::model_router_import_codexmanager_data_preserving_target(source_path),
            )
        }
        _ => return None,
    };

    Some(super::response(req, result))
}
