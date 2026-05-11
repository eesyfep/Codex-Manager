use codexmanager_core::rpc::types::{
    ApiKeyListResult, ApiKeyUsageStatListResult, JsonRpcRequest, JsonRpcResponse,
    ManagedModelCatalogUpsertParams,
};

use crate::{
    apikey_create, apikey_delete, apikey_disable, apikey_enable, apikey_list, apikey_models,
    apikey_read_secret, apikey_update_model, apikey_usage_stats,
};

/// 函数 `try_handle`
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
pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "apikey/list" => super::value_or_error(
            apikey_list::read_api_keys().map(|items| ApiKeyListResult { items }),
        ),
        "apikey/create" => {
            let name = super::string_param(req, "name");
            let model_slug = super::string_array_param(req, "modelSlugs")
                .and_then(crate::apikey::model_binding::serialize_model_bindings)
                .or_else(|| super::string_param(req, "modelSlug"));
            let reasoning_effort = super::string_param(req, "reasoningEffort");
            let service_tier = super::string_param(req, "serviceTier");
            let protocol_type = super::string_param(req, "protocolType");
            let upstream_base_url = super::string_param(req, "upstreamBaseUrl");
            let static_headers_json = super::string_param(req, "staticHeadersJson");
            let rotation_strategy = super::string_param(req, "rotationStrategy");
            let aggregate_api_id = super::string_param(req, "aggregateApiId");
            let account_plan_filter = super::string_param(req, "accountPlanFilter");
            super::value_or_error(apikey_create::create_api_key(
                name,
                model_slug,
                reasoning_effort,
                service_tier,
                protocol_type,
                upstream_base_url,
                static_headers_json,
                rotation_strategy,
                aggregate_api_id,
                account_plan_filter,
            ))
        }
        "apikey/readSecret" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            super::value_or_error(apikey_read_secret::read_api_key_secret(key_id))
        }
        "apikey/models" => {
            let refresh_remote = super::bool_param(req, "refreshRemote").unwrap_or(false);
            super::value_or_error(apikey_models::read_model_options(refresh_remote))
        }
        "apikey/modelCatalogList" => {
            let refresh_remote = super::bool_param(req, "refreshRemote").unwrap_or(false);
            super::value_or_error(apikey_models::read_managed_model_catalog(refresh_remote))
        }
        "apikey/modelCatalogSave" => {
            let params = req
                .params
                .clone()
                .ok_or_else(|| "缺少模型参数".to_string())
                .and_then(|value| {
                    serde_json::from_value::<ManagedModelCatalogUpsertParams>(value)
                        .map_err(|err| format!("解析模型参数失败: {err}"))
                });
            super::value_or_error(params.and_then(apikey_models::save_managed_model_catalog_model))
        }
        "apikey/modelCatalogDelete" => {
            let slug = super::str_param(req, "slug").unwrap_or("");
            super::ok_or_error(apikey_models::delete_managed_model_catalog_model(slug))
        }
        "apikey/usageStats" => super::value_or_error(
            apikey_usage_stats::read_api_key_usage_stats()
                .map(|items| ApiKeyUsageStatListResult { items }),
        ),
        "apikey/updateModel" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            let has_name = req
                .params
                .as_ref()
                .and_then(|value| value.as_object())
                .map(|params| params.contains_key("name"))
                .unwrap_or(false);
            let name = super::string_param(req, "name");
            let model_slug = super::string_array_param(req, "modelSlugs")
                .and_then(crate::apikey::model_binding::serialize_model_bindings)
                .or_else(|| super::string_param(req, "modelSlug"));
            let reasoning_effort = super::string_param(req, "reasoningEffort");
            let service_tier = super::string_param(req, "serviceTier");
            let protocol_type = super::string_param(req, "protocolType");
            let upstream_base_url = super::string_param(req, "upstreamBaseUrl");
            let static_headers_json = super::string_param(req, "staticHeadersJson");
            let rotation_strategy = super::string_param(req, "rotationStrategy");
            let aggregate_api_id = super::string_param(req, "aggregateApiId");
            let account_plan_filter = super::string_param(req, "accountPlanFilter");
            super::ok_or_error(apikey_update_model::update_api_key_model(
                key_id,
                name,
                has_name,
                model_slug,
                reasoning_effort,
                service_tier,
                protocol_type,
                upstream_base_url,
                static_headers_json,
                rotation_strategy,
                aggregate_api_id,
                account_plan_filter,
            ))
        }
        "apikey/delete" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            super::ok_or_error(apikey_delete::delete_api_key(key_id))
        }
        "apikey/disable" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            super::ok_or_error(apikey_disable::disable_api_key(key_id))
        }
        "apikey/enable" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            super::ok_or_error(apikey_enable::enable_api_key(key_id))
        }
        _ => return None,
    };

    Some(super::response(req, result))
}
