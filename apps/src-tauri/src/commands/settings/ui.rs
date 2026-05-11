use crate::app_storage::{apply_runtime_storage_env, profile_service_addr_override};

use super::tray_state::{
    effective_close_to_tray_requested, sync_window_runtime_state_from_settings, tray_available,
};

/// 函数 `app_close_to_tray_on_close_get`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub fn app_close_to_tray_on_close_get(app: tauri::AppHandle) -> bool {
    apply_runtime_storage_env(&app);
    if let Ok(mut settings) = codexmanager_service::app_settings_get() {
        sync_window_runtime_state_from_settings(&mut settings);
    }
    codexmanager_service::current_close_to_tray_on_close_setting() && tray_available()
}

/// 函数 `app_close_to_tray_on_close_set`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
/// - enabled: 参数 enabled
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub fn app_close_to_tray_on_close_set(app: tauri::AppHandle, enabled: bool) -> bool {
    apply_runtime_storage_env(&app);
    let payload = serde_json::json!({
        "closeToTrayOnClose": enabled
    });
    if let Ok(mut settings) = codexmanager_service::app_settings_set(Some(&payload)) {
        sync_window_runtime_state_from_settings(&mut settings);
    }
    codexmanager_service::current_close_to_tray_on_close_setting() && tray_available()
}

/// 函数 `app_settings_get`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn app_settings_get(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    apply_runtime_storage_env(&app);
    let service_addr_override = profile_service_addr_override(&app);
    let mut settings = tauri::async_runtime::spawn_blocking(move || {
        codexmanager_service::app_settings_get_with_overrides(
            Some(effective_close_to_tray_requested()),
            Some(tray_available()),
        )
    })
    .await
    .map_err(|err| format!("app_settings_get task failed: {err}"))??;
    if let Some(addr) = service_addr_override {
        settings["serviceAddr"] = serde_json::Value::String(addr.to_string());
    }
    sync_window_runtime_state_from_settings(&mut settings);
    Ok(settings)
}

/// 函数 `app_settings_set`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
/// - patch: 参数 patch
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn app_settings_set(
    app: tauri::AppHandle,
    mut patch: serde_json::Value,
) -> Result<serde_json::Value, String> {
    apply_runtime_storage_env(&app);
    let service_addr_override = profile_service_addr_override(&app);
    if service_addr_override.is_some() {
        if let Some(obj) = patch.as_object_mut() {
            obj.remove("serviceAddr");
            if let Some(nested) = obj.get_mut("patch").and_then(|value| value.as_object_mut()) {
                nested.remove("serviceAddr");
            }
        }
    }
    let mut settings = tauri::async_runtime::spawn_blocking(move || {
        codexmanager_service::app_settings_set(Some(&patch))
    })
    .await
    .map_err(|err| format!("app_settings_set task failed: {err}"))??;
    if let Some(addr) = service_addr_override {
        settings["serviceAddr"] = serde_json::Value::String(addr.to_string());
    }
    sync_window_runtime_state_from_settings(&mut settings);
    Ok(settings)
}
