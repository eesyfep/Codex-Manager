use codexmanager_core::storage::Storage;
use rand::RngCore;
use rusqlite::{backup::Backup, Connection};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
#[cfg(test)]
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct CachedStorage {
    path: String,
    storage: Storage,
}

thread_local! {
    static STORAGE_CACHE: RefCell<Option<CachedStorage>> = const { RefCell::new(None) };
}

pub(crate) struct StorageHandle {
    path: String,
    storage: Option<Storage>,
}

impl StorageHandle {
    /// 函数 `new`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - path: 参数 path
    /// - storage: 参数 storage
    ///
    /// # 返回
    /// 返回函数执行结果
    fn new(path: String, storage: Storage) -> Self {
        Self {
            path,
            storage: Some(storage),
        }
    }
}

impl Deref for StorageHandle {
    type Target = Storage;

    /// 函数 `deref`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    fn deref(&self) -> &Self::Target {
        self.storage.as_ref().expect("storage handle should exist")
    }
}

impl DerefMut for StorageHandle {
    /// 函数 `deref_mut`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.storage.as_mut().expect("storage handle should exist")
    }
}

impl Drop for StorageHandle {
    /// 函数 `drop`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 无
    fn drop(&mut self) {
        let Some(storage) = self.storage.take() else {
            return;
        };
        let path = self.path.clone();
        STORAGE_CACHE.with(|cell| {
            let mut cache = cell.borrow_mut();
            *cache = Some(CachedStorage { path, storage });
        });
    }
}

/// 函数 `normalize_key_part`
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
fn normalize_key_part(value: Option<&str>) -> Option<String> {
    // 规范化 key 片段，去除空白并避免分隔符冲突
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.replace("::", "_"))
}

/// 函数 `compact_key_part`
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
fn compact_key_part(value: &str) -> String {
    // 对过长/复杂后缀做短哈希，避免账号ID过长且保留稳定唯一性。
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let should_hash = trimmed.len() > 16
        || trimmed.contains('|')
        || trimmed.contains('-')
        || trimmed.contains(' ');
    if !should_hash {
        return trimmed.to_string();
    }
    let mut hasher = Sha256::new();
    hasher.update(trimmed.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(12);
    for b in digest.iter().take(6) {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// 函数 `account_key`
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
pub(crate) fn account_key(account_id: &str, tags: Option<&str>) -> String {
    // 组合账号与标签，生成稳定的账户唯一标识
    let mut parts = Vec::new();
    parts.push(account_id.to_string());
    if let Some(value) = normalize_key_part(tags) {
        let compact = compact_key_part(&value);
        if !compact.is_empty() {
            parts.push(compact);
        }
    }
    parts.join("::")
}

/// 函数 `hash_platform_key`
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
pub(crate) fn hash_platform_key(key: &str) -> String {
    // 对平台 Key 做不可逆哈希，避免明文存储
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// 函数 `generate_platform_key`
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
pub(crate) fn generate_platform_key() -> String {
    // 生成随机平台 Key（十六进制）
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let mut out = String::with_capacity(buf.len() * 2);
    for b in buf {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// 函数 `generate_key_id`
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
pub(crate) fn generate_key_id() -> String {
    // 生成短 ID 作为平台 Key 的展示标识
    let mut buf = [0u8; 6];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let mut out = String::from("gk_");
    for b in buf {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// 函数 `generate_aggregate_api_id`
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
pub(crate) fn generate_aggregate_api_id() -> String {
    let mut buf = [0u8; 6];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let mut out = String::from("ag_");
    for b in buf {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

pub(crate) fn generate_model_router_id(prefix: &str) -> String {
    let mut buf = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let normalized_prefix = prefix.trim().trim_end_matches('_');
    let mut out = if normalized_prefix.is_empty() {
        String::from("mr_")
    } else {
        format!("{normalized_prefix}_")
    };
    for b in buf {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

#[cfg(test)]
static STORAGE_OPEN_COUNTS: std::sync::OnceLock<std::sync::Mutex<HashMap<String, usize>>> =
    std::sync::OnceLock::new();

/// 函数 `open_storage`
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
pub(crate) fn open_storage() -> Option<StorageHandle> {
    // 读取数据库路径并打开存储
    let path = match std::env::var("CODEXMANAGER_DB_PATH") {
        Ok(path) => path,
        Err(_) => {
            log::warn!("CODEXMANAGER_DB_PATH not set");
            return None;
        }
    };
    open_storage_at_path(&path)
}

/// 函数 `open_storage_at_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn open_storage_at_path(path: &str) -> Option<StorageHandle> {
    if let Some(storage) = take_cached_storage(&path) {
        return Some(StorageHandle::new(path.to_string(), storage));
    }

    if !Path::new(&path).exists() {
        log::warn!("storage path missing: {}", path);
    }
    let storage = match Storage::open(&path) {
        Ok(storage) => storage,
        Err(err) => {
            log::error!("open storage failed: {} ({})", path, err);
            return None;
        }
    };
    #[cfg(test)]
    record_storage_open_for_tests(path);
    Some(StorageHandle::new(path.to_string(), storage))
}

/// 函数 `initialize_storage`
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
pub(crate) fn initialize_storage() -> Result<(), String> {
    let path = std::env::var("CODEXMANAGER_DB_PATH")
        .map_err(|_| "CODEXMANAGER_DB_PATH not set".to_string())?;
    if !Path::new(&path).exists() {
        log::warn!("storage path missing: {}", path);
    }
    backup_before_model_router_migration_if_needed(Path::new(&path))?;
    let storage =
        Storage::open(&path).map_err(|err| format!("open storage failed: {} ({})", path, err))?;
    storage
        .init()
        .map_err(|err| format!("storage init failed: {} ({})", path, err))?;
    Ok(())
}

pub(crate) fn storage_is_empty(path: &Path) -> Result<bool, String> {
    let storage = Storage::open(path).map_err(|err| {
        format!(
            "open storage for empty check failed: {} ({})",
            path.display(),
            err
        )
    })?;
    storage
        .init()
        .map_err(|err| format!("storage init failed: {} ({})", path.display(), err))?;
    let account_count = storage
        .list_accounts()
        .map_err(|err| format!("count accounts failed: {err}"))?
        .len();
    let api_key_count = storage
        .list_api_keys()
        .map_err(|err| format!("count api keys failed: {err}"))?
        .len();
    let aggregate_api_count = storage
        .list_aggregate_apis()
        .map_err(|err| format!("count aggregate apis failed: {err}"))?
        .len();
    let request_log_count = storage
        .count_request_logs(None, None, None, None)
        .map_err(|err| format!("count request logs failed: {err}"))?;
    Ok(account_count == 0
        && api_key_count == 0
        && aggregate_api_count == 0
        && request_log_count == 0)
}

fn backup_before_model_router_migration_if_needed(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Ok(());
    }
    if migration_already_applied(path, "053_model_router")? {
        return Ok(());
    }
    let backup_path = migration_backup_path(path, "053_model_router");
    copy_sqlite_snapshot(path, &backup_path)?;
    log::info!(
        "created pre-migration backup for 053_model_router: {} -> {}",
        path.display(),
        backup_path.display()
    );
    Ok(())
}

fn migration_already_applied(path: &Path, version: &str) -> Result<bool, String> {
    let conn = Connection::open(path).map_err(|err| {
        format!(
            "open storage for migration check failed: {} ({})",
            path.display(),
            err
        )
    })?;
    conn.busy_timeout(Duration::from_millis(3000))
        .map_err(|err| {
            format!(
                "configure migration check failed: {} ({})",
                path.display(),
                err
            )
        })?;
    let has_table = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if !has_table {
        return Ok(false);
    }
    let applied = conn
        .query_row(
            "SELECT 1 FROM schema_migrations WHERE version = ?1 LIMIT 1",
            [version],
            |_| Ok(()),
        )
        .is_ok();
    Ok(applied)
}

fn migration_backup_path(path: &Path, migration: &str) -> PathBuf {
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
    parent.join(format!("{stem}.{migration}.{ts}.bak.db"))
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

/// 函数 `take_cached_storage`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
fn take_cached_storage(path: &str) -> Option<Storage> {
    STORAGE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        match cache.take() {
            Some(CachedStorage {
                path: cached_path,
                storage,
            }) if cached_path == path => Some(storage),
            Some(other) => {
                *cache = Some(other);
                None
            }
            None => None,
        }
    })
}

/// 函数 `clear_storage_cache_for_tests`
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
#[cfg(test)]
fn clear_storage_cache_for_tests() {
    STORAGE_CACHE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// 函数 `record_storage_open_for_tests`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 无
#[cfg(test)]
fn record_storage_open_for_tests(path: &str) {
    let mutex = STORAGE_OPEN_COUNTS.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut counts = mutex.lock().unwrap_or_else(|poisoned| {
        log::warn!("storage open count lock poisoned; recovering for tests");
        poisoned.into_inner()
    });
    let entry = counts.entry(path.to_string()).or_insert(0);
    *entry += 1;
}

/// 函数 `storage_open_count_for_tests`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn storage_open_count_for_tests(path: &str) -> usize {
    let Some(mutex) = STORAGE_OPEN_COUNTS.get() else {
        return 0;
    };
    let counts = mutex.lock().unwrap_or_else(|poisoned| {
        log::warn!("storage open count lock poisoned; recovering for tests");
        poisoned.into_inner()
    });
    counts.get(path).copied().unwrap_or(0)
}

/// 函数 `clear_storage_open_count_for_tests`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 无
#[cfg(test)]
fn clear_storage_open_count_for_tests(path: &str) {
    let Some(mutex) = STORAGE_OPEN_COUNTS.get() else {
        return;
    };
    let mut counts = mutex.lock().unwrap_or_else(|poisoned| {
        log::warn!("storage open count lock poisoned; recovering for tests");
        poisoned.into_inner()
    });
    counts.remove(path);
}

#[cfg(test)]
#[path = "tests/storage_helpers_tests.rs"]
mod tests;
