use super::{
    clear_storage_cache_for_tests, clear_storage_open_count_for_tests, initialize_storage,
    open_storage_at_path, storage_open_count_for_tests,
};
use codexmanager_core::storage::Storage;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// 函数 `unique_db_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - prefix: 参数 prefix
///
/// # 返回
/// 返回函数执行结果
fn unique_db_path(prefix: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("{prefix}-{nonce}.db"))
        .to_string_lossy()
        .to_string()
}

fn model_router_backup_paths(db_path: &Path) -> Vec<PathBuf> {
    let Some(parent) = db_path.parent() else {
        return Vec::new();
    };
    let Some(stem) = db_path.file_stem().and_then(|value| value.to_str()) else {
        return Vec::new();
    };
    let prefix = format!("{stem}.053_model_router.");
    std::fs::read_dir(parent)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".bak.db"))
        })
        .collect()
}

/// 函数 `open_storage_reuses_cached_connection_in_same_thread`
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
fn open_storage_reuses_cached_connection_in_same_thread() {
    let db_path = unique_db_path("codexmanager-open-storage-reuse");
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);

    let storage = open_storage_at_path(&db_path).expect("open storage 1");
    storage.init().expect("init");
    drop(storage);

    let storage = open_storage_at_path(&db_path).expect("open storage 2");
    drop(storage);

    assert_eq!(storage_open_count_for_tests(&db_path), 1);

    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);
    let _ = std::fs::remove_file(&db_path);
}

/// 函数 `open_storage_reopens_when_db_path_changes`
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
fn open_storage_reopens_when_db_path_changes() {
    let db_path_1 = unique_db_path("codexmanager-open-storage-path-1");
    let db_path_2 = unique_db_path("codexmanager-open-storage-path-2");
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path_1);
    clear_storage_open_count_for_tests(&db_path_2);

    let storage = open_storage_at_path(&db_path_1).expect("open storage path 1");
    storage.init().expect("init 1");
    drop(storage);

    let storage = open_storage_at_path(&db_path_2).expect("open storage path 2");
    storage.init().expect("init 2");
    drop(storage);

    assert_eq!(storage_open_count_for_tests(&db_path_1), 1);
    assert_eq!(storage_open_count_for_tests(&db_path_2), 1);

    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path_1);
    clear_storage_open_count_for_tests(&db_path_2);
    let _ = std::fs::remove_file(&db_path_1);
    let _ = std::fs::remove_file(&db_path_2);
}

#[test]
fn initialize_storage_backs_up_before_model_router_migration() {
    let db_path = PathBuf::from(unique_db_path("codexmanager-model-router-backup"));
    let storage = Storage::open(&db_path).expect("open storage");
    storage.init().expect("init storage");
    drop(storage);

    let conn = Connection::open(&db_path).expect("open db");
    conn.execute(
        "DELETE FROM schema_migrations WHERE version = '053_model_router'",
        [],
    )
    .expect("remove migration marker");
    drop(conn);

    assert!(model_router_backup_paths(&db_path).is_empty());
    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    initialize_storage().expect("initialize storage");

    let backup_paths = model_router_backup_paths(&db_path);
    assert_eq!(backup_paths.len(), 1);
    let conn = Connection::open(&db_path).expect("open migrated db");
    let marker_exists = conn
        .query_row(
            "SELECT 1 FROM schema_migrations WHERE version = '053_model_router' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .is_ok();
    assert!(marker_exists);

    clear_storage_cache_for_tests();
    let _ = std::fs::remove_file(&db_path);
    for backup_path in backup_paths {
        let _ = std::fs::remove_file(backup_path);
    }
}
