use codexmanager_core::rpc::types::{
    AccountListParams, DashboardDailyTokenUsageBucket, StartupSnapshotResult,
};

use crate::{
    account_list, apikey_list, apikey_models, gateway, requestlog_list, requestlog_today_summary,
    storage_helpers, usage_aggregate, usage_list,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct StartupSnapshotOptions {
    pub include_api_models: bool,
    pub include_request_logs: bool,
    pub include_dashboard_usage: bool,
}

impl Default for StartupSnapshotOptions {
    fn default() -> Self {
        Self {
            include_api_models: true,
            include_request_logs: true,
            include_dashboard_usage: true,
        }
    }
}

/// 函数 `read_startup_snapshot`
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
pub(crate) fn read_startup_snapshot(
    request_log_limit: Option<i64>,
    day_start_ts: Option<i64>,
    day_end_ts: Option<i64>,
    options: StartupSnapshotOptions,
) -> Result<StartupSnapshotResult, String> {
    let accounts = account_list::read_accounts(AccountListParams::default(), false)?.items;
    let usage_snapshots = usage_list::read_usage_snapshots()?;
    let usage_aggregate_summary = usage_aggregate::read_usage_aggregate_summary()?;
    let api_keys = apikey_list::read_api_keys()?;
    let api_models = if options.include_api_models {
        apikey_models::read_model_options(false)?
    } else {
        Default::default()
    };
    let manual_preferred_account_id = gateway::manual_preferred_account();
    let request_log_today_summary =
        requestlog_today_summary::read_requestlog_today_summary(day_start_ts, day_end_ts)?;
    let request_logs = if options.include_request_logs {
        requestlog_list::read_request_logs(None, request_log_limit)?
    } else {
        Vec::new()
    };
    let dashboard_token_usage = if options.include_dashboard_usage {
        read_dashboard_token_usage(day_start_ts, day_end_ts).unwrap_or_else(|err| {
            log::warn!("startup snapshot dashboard token usage unavailable: {err}");
            Vec::new()
        })
    } else {
        Vec::new()
    };
    let dashboard_daily_token_usage = if options.include_dashboard_usage {
        read_dashboard_daily_token_usage().unwrap_or_else(|err| {
            log::warn!("startup snapshot daily token usage unavailable: {err}");
            Vec::new()
        })
    } else {
        Vec::new()
    };

    Ok(StartupSnapshotResult {
        accounts,
        usage_snapshots,
        usage_aggregate_summary,
        api_keys,
        api_models,
        manual_preferred_account_id,
        request_log_today_summary,
        request_logs,
        dashboard_token_usage,
        dashboard_daily_token_usage,
    })
}

fn read_dashboard_token_usage(
    day_start_ts: Option<i64>,
    day_end_ts: Option<i64>,
) -> Result<Vec<codexmanager_core::rpc::types::DashboardTokenUsageSummary>, String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .summarize_dashboard_token_usage(day_start_ts, day_end_ts, 64)
        .map_err(|err| err.to_string())
        .map(|items| {
            items
                .into_iter()
                .map(
                    |item| codexmanager_core::rpc::types::DashboardTokenUsageSummary {
                        key_id: item.key_id,
                        key_name: item.key_name,
                        account_id: item.account_id,
                        account_label: item.account_label,
                        aggregate_api_id: item.aggregate_api_id,
                        aggregate_api_supplier_name: item.aggregate_api_supplier_name,
                        aggregate_api_url: item.aggregate_api_url,
                        model: item.model,
                        request_count: item.request_count,
                        input_tokens: item.input_tokens,
                        cached_input_tokens: item.cached_input_tokens,
                        output_tokens: item.output_tokens,
                        reasoning_output_tokens: item.reasoning_output_tokens,
                        total_tokens: item.total_tokens,
                        estimated_cost_usd: item.estimated_cost_usd,
                        last_used_at: item.last_used_at,
                    },
                )
                .collect()
        })
}

fn read_dashboard_daily_token_usage() -> Result<Vec<DashboardDailyTokenUsageBucket>, String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let now = codexmanager_core::storage::now_ts();
    let start_ts = now.saturating_sub(90 * 86_400);
    let end_ts = now.saturating_add(86_400);
    storage
        .summarize_dashboard_daily_token_usage(start_ts, end_ts, 90)
        .map_err(|err| err.to_string())
        .map(|items| {
            items
                .into_iter()
                .map(|item| DashboardDailyTokenUsageBucket {
                    day_start_ts: item.day_start_ts,
                    source_key: item.source_key,
                    source_label: item.source_label,
                    model: item.model,
                    billable_input_tokens: item.billable_input_tokens,
                    request_count: item.request_count,
                    input_tokens: item.input_tokens,
                    cached_input_tokens: item.cached_input_tokens,
                    output_tokens: item.output_tokens,
                    reasoning_output_tokens: item.reasoning_output_tokens,
                    total_tokens: item.total_tokens,
                    estimated_cost_usd: item.estimated_cost_usd,
                })
                .collect()
        })
}
