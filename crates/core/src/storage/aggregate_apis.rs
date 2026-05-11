use rusqlite::{params, Result, Row};

use super::{now_ts, AggregateApi, Storage};

const AGGREGATE_API_SELECT_SQL: &str = "SELECT
    id,
    provider_type,
    supplier_name,
    sort,
    url,
    auth_type,
    auth_params_json,
    action,
    pool,
    wool_max_inflight,
    wool_cooldown_until,
    wool_failure_count,
    wool_last_preflight_at,
    fast,
    compatibility_mode,
    status,
    created_at,
    updated_at,
    last_test_at,
    last_test_status,
    last_test_error
 FROM aggregate_apis";

impl Storage {
    /// 函数 `insert_aggregate_api`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - api: 参数 api
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn insert_aggregate_api(&self, api: &AggregateApi) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO aggregate_apis (
                id,
                provider_type,
                supplier_name,
                sort,
                url,
                auth_type,
                auth_params_json,
                action,
                pool,
                wool_max_inflight,
                wool_cooldown_until,
                wool_failure_count,
                wool_last_preflight_at,
                fast,
                compatibility_mode,
                status,
                created_at,
                updated_at,
                last_test_at,
                last_test_status,
                last_test_error
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
            params![
                &api.id,
                &api.provider_type,
                &api.supplier_name,
                api.sort,
                &api.url,
                &api.auth_type,
                &api.auth_params_json,
                &api.action,
                &api.pool,
                &api.wool_max_inflight,
                &api.wool_cooldown_until,
                api.wool_failure_count,
                &api.wool_last_preflight_at,
                api.fast,
                api.compatibility_mode,
                &api.status,
                api.created_at,
                api.updated_at,
                &api.last_test_at,
                &api.last_test_status,
                &api.last_test_error,
            ],
        )?;
        Ok(())
    }

    /// 函数 `list_aggregate_apis`
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
    pub fn list_aggregate_apis(&self) -> Result<Vec<AggregateApi>> {
        let mut stmt = self.conn.prepare(&format!(
            "{AGGREGATE_API_SELECT_SQL} ORDER BY sort ASC, updated_at DESC"
        ))?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_aggregate_api_row(row)?);
        }
        Ok(out)
    }

    /// 函数 `find_aggregate_api_by_id`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - api_id: 参数 api_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn find_aggregate_api_by_id(&self, api_id: &str) -> Result<Option<AggregateApi>> {
        let mut stmt = self.conn.prepare(&format!(
            "{AGGREGATE_API_SELECT_SQL}
             WHERE id = ?1
             LIMIT 1"
        ))?;
        let mut rows = stmt.query([api_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_aggregate_api_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// 函数 `update_aggregate_api`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - api_id: 参数 api_id
    /// - url: 参数 url
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_aggregate_api(&self, api_id: &str, url: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE aggregate_apis SET url = ?1, updated_at = ?2 WHERE id = ?3",
            (url, now_ts(), api_id),
        )?;
        Ok(())
    }

    /// 函数 `update_aggregate_api_supplier_name`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - api_id: 参数 api_id
    /// - supplier_name: 参数 supplier_name
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_aggregate_api_supplier_name(
        &self,
        api_id: &str,
        supplier_name: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE aggregate_apis SET supplier_name = ?1, updated_at = ?2 WHERE id = ?3",
            (supplier_name, now_ts(), api_id),
        )?;
        Ok(())
    }

    /// 函数 `update_aggregate_api_sort`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - api_id: 参数 api_id
    /// - sort: 参数 sort
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_aggregate_api_sort(&self, api_id: &str, sort: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE aggregate_apis SET sort = ?1, updated_at = ?2 WHERE id = ?3",
            (sort, now_ts(), api_id),
        )?;
        Ok(())
    }

    pub fn update_aggregate_api_status(&self, api_id: &str, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE aggregate_apis SET status = ?1, updated_at = ?2 WHERE id = ?3",
            (status, now_ts(), api_id),
        )?;
        Ok(())
    }

    /// 函数 `update_aggregate_api_type`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - api_id: 参数 api_id
    /// - provider_type: 参数 provider_type
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_aggregate_api_type(&self, api_id: &str, provider_type: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE aggregate_apis SET provider_type = ?1, updated_at = ?2 WHERE id = ?3",
            (provider_type, now_ts(), api_id),
        )?;
        Ok(())
    }

    pub fn update_aggregate_api_auth_type(&self, api_id: &str, auth_type: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE aggregate_apis SET auth_type = ?1, updated_at = ?2 WHERE id = ?3",
            (auth_type, now_ts(), api_id),
        )?;
        Ok(())
    }

    pub fn update_aggregate_api_auth_params_json(
        &self,
        api_id: &str,
        auth_params_json: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE aggregate_apis SET auth_params_json = ?1, updated_at = ?2 WHERE id = ?3",
            (auth_params_json, now_ts(), api_id),
        )?;
        Ok(())
    }

    pub fn update_aggregate_api_action(&self, api_id: &str, action: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE aggregate_apis SET action = ?1, updated_at = ?2 WHERE id = ?3",
            (action, now_ts(), api_id),
        )?;
        Ok(())
    }

    pub fn update_aggregate_api_pool(&self, api_id: &str, pool: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE aggregate_apis SET pool = ?1, updated_at = ?2 WHERE id = ?3",
            (pool, now_ts(), api_id),
        )?;
        Ok(())
    }

    pub fn update_aggregate_api_wool_max_inflight(
        &self,
        api_id: &str,
        wool_max_inflight: Option<i64>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE aggregate_apis SET wool_max_inflight = ?1, updated_at = ?2 WHERE id = ?3",
            (wool_max_inflight, now_ts(), api_id),
        )?;
        Ok(())
    }

    pub fn update_aggregate_api_fast(&self, api_id: &str, fast: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE aggregate_apis SET fast = ?1, updated_at = ?2 WHERE id = ?3",
            (fast, now_ts(), api_id),
        )?;
        Ok(())
    }

    pub fn update_aggregate_api_compatibility_mode(
        &self,
        api_id: &str,
        compatibility_mode: bool,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE aggregate_apis SET compatibility_mode = ?1, updated_at = ?2 WHERE id = ?3",
            (compatibility_mode, now_ts(), api_id),
        )?;
        Ok(())
    }

    pub fn mark_aggregate_api_wool_preflight_success(&self, api_id: &str) -> Result<()> {
        let now = now_ts();
        self.conn.execute(
            "UPDATE aggregate_apis
             SET wool_last_preflight_at = ?1,
                 wool_failure_count = 0,
                 wool_cooldown_until = NULL,
                 updated_at = ?1
             WHERE id = ?2",
            (now, api_id),
        )?;
        Ok(())
    }

    pub fn mark_aggregate_api_wool_failure(
        &self,
        api_id: &str,
        cooldown_until: Option<i64>,
    ) -> Result<()> {
        let now = now_ts();
        self.conn.execute(
            "UPDATE aggregate_apis
             SET wool_failure_count = COALESCE(wool_failure_count, 0) + 1,
                 wool_cooldown_until = ?1,
                 updated_at = ?2
             WHERE id = ?3",
            (cooldown_until, now, api_id),
        )?;
        Ok(())
    }

    /// 函数 `delete_aggregate_api`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - api_id: 参数 api_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn delete_aggregate_api(&self, api_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM aggregate_api_secrets WHERE aggregate_api_id = ?1",
            [api_id],
        )?;
        self.conn
            .execute("DELETE FROM aggregate_apis WHERE id = ?1", [api_id])?;
        Ok(())
    }

    /// 函数 `upsert_aggregate_api_secret`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - api_id: 参数 api_id
    /// - secret_value: 参数 secret_value
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn upsert_aggregate_api_secret(&self, api_id: &str, secret_value: &str) -> Result<()> {
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO aggregate_api_secrets (aggregate_api_id, secret_value, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(aggregate_api_id) DO UPDATE SET
               secret_value = excluded.secret_value,
               updated_at = excluded.updated_at",
            (api_id, secret_value, now),
        )?;
        Ok(())
    }

    /// 函数 `find_aggregate_api_secret_by_id`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - api_id: 参数 api_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn find_aggregate_api_secret_by_id(&self, api_id: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT secret_value FROM aggregate_api_secrets WHERE aggregate_api_id = ?1 LIMIT 1",
        )?;
        let mut rows = stmt.query([api_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// 函数 `update_aggregate_api_test_result`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - api_id: 参数 api_id
    /// - ok: 参数 ok
    /// - status_code: 参数 status_code
    /// - error: 参数 error
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_aggregate_api_test_result(
        &self,
        api_id: &str,
        ok: bool,
        status_code: Option<i64>,
        error: Option<&str>,
    ) -> Result<()> {
        let now = now_ts();
        let last_test_status = if ok { Some("success") } else { Some("failed") };
        self.conn.execute(
            "UPDATE aggregate_apis
             SET last_test_at = ?1,
                 last_test_status = ?2,
                 last_test_error = ?3,
                 updated_at = ?1
             WHERE id = ?4",
            (now, last_test_status, error, api_id),
        )?;
        if let Some(code) = status_code {
            if !ok {
                let message = format!("http_status={code}");
                self.conn.execute(
                    "UPDATE aggregate_apis SET last_test_error = ?1 WHERE id = ?2",
                    (message, api_id),
                )?;
            }
        }
        Ok(())
    }

    /// 函数 `ensure_aggregate_apis_table`
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
    pub(super) fn ensure_aggregate_apis_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS aggregate_apis (
                id TEXT PRIMARY KEY,
                provider_type TEXT NOT NULL DEFAULT 'codex',
                supplier_name TEXT,
                sort INTEGER NOT NULL DEFAULT 0,
                url TEXT NOT NULL,
                auth_type TEXT NOT NULL DEFAULT 'apikey',
                auth_params_json TEXT,
                action TEXT,
                pool TEXT NOT NULL DEFAULT 'primary',
                wool_max_inflight INTEGER,
                wool_cooldown_until INTEGER,
                wool_failure_count INTEGER NOT NULL DEFAULT 0,
                wool_last_preflight_at INTEGER,
                fast INTEGER NOT NULL DEFAULT 0,
                compatibility_mode INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'active',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_test_at INTEGER,
                last_test_status TEXT,
                last_test_error TEXT
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_aggregate_apis_created_at ON aggregate_apis(created_at DESC)",
            [],
        )?;
        self.ensure_column("aggregate_apis", "provider_type", "TEXT")?;
        self.ensure_column("aggregate_apis", "supplier_name", "TEXT")?;
        self.ensure_column("aggregate_apis", "sort", "INTEGER DEFAULT 0")?;
        self.ensure_column(
            "aggregate_apis",
            "auth_type",
            "TEXT NOT NULL DEFAULT 'apikey'",
        )?;
        self.ensure_column("aggregate_apis", "auth_params_json", "TEXT")?;
        self.ensure_column("aggregate_apis", "action", "TEXT")?;
        self.ensure_aggregate_api_wool_columns()?;
        self.ensure_aggregate_api_fast_column()?;
        self.ensure_aggregate_api_compatibility_mode_column()?;
        self.conn.execute(
            "UPDATE aggregate_apis
             SET provider_type = COALESCE(NULLIF(TRIM(provider_type), ''), 'codex')
             WHERE provider_type IS NULL OR TRIM(provider_type) = ''",
            [],
        )?;
        self.conn.execute(
            "UPDATE aggregate_apis
             SET auth_type = COALESCE(NULLIF(TRIM(auth_type), ''), 'apikey')
             WHERE auth_type IS NULL OR TRIM(auth_type) = ''",
            [],
        )?;
        self.conn.execute(
            "UPDATE aggregate_apis
             SET sort = COALESCE(sort, 0)
             WHERE sort IS NULL",
            [],
        )?;
        Ok(())
    }

    pub(super) fn ensure_aggregate_api_wool_columns(&self) -> Result<()> {
        self.ensure_column("aggregate_apis", "pool", "TEXT NOT NULL DEFAULT 'primary'")?;
        self.ensure_column("aggregate_apis", "wool_max_inflight", "INTEGER")?;
        self.ensure_column("aggregate_apis", "wool_cooldown_until", "INTEGER")?;
        self.ensure_column(
            "aggregate_apis",
            "wool_failure_count",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column("aggregate_apis", "wool_last_preflight_at", "INTEGER")?;
        self.conn.execute(
            "UPDATE aggregate_apis
             SET pool = 'primary'
             WHERE pool IS NULL OR TRIM(pool) = '' OR pool NOT IN ('primary', 'wool')",
            [],
        )?;
        self.conn.execute(
            "UPDATE aggregate_apis
             SET wool_failure_count = COALESCE(wool_failure_count, 0)
             WHERE wool_failure_count IS NULL",
            [],
        )?;
        Ok(())
    }

    pub(super) fn ensure_aggregate_api_fast_column(&self) -> Result<()> {
        self.ensure_column("aggregate_apis", "fast", "INTEGER NOT NULL DEFAULT 0")?;
        self.conn.execute(
            "UPDATE aggregate_apis
             SET fast = COALESCE(fast, 0)
             WHERE fast IS NULL",
            [],
        )?;
        Ok(())
    }

    pub(super) fn ensure_aggregate_api_compatibility_mode_column(&self) -> Result<()> {
        self.ensure_column(
            "aggregate_apis",
            "compatibility_mode",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.conn.execute(
            "UPDATE aggregate_apis
             SET compatibility_mode = COALESCE(compatibility_mode, 0)
             WHERE compatibility_mode IS NULL",
            [],
        )?;
        Ok(())
    }

    /// 函数 `ensure_aggregate_api_secrets_table`
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
    pub(super) fn ensure_aggregate_api_secrets_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS aggregate_api_secrets (
                aggregate_api_id TEXT PRIMARY KEY REFERENCES aggregate_apis(id) ON DELETE CASCADE,
                secret_value TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_aggregate_api_secrets_updated_at ON aggregate_api_secrets(updated_at)",
            [],
        )?;
        Ok(())
    }
}

/// 函数 `map_aggregate_api_row`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - row: 参数 row
///
/// # 返回
/// 返回函数执行结果
fn map_aggregate_api_row(row: &Row<'_>) -> Result<AggregateApi> {
    Ok(AggregateApi {
        id: row.get(0)?,
        provider_type: row.get(1)?,
        supplier_name: row.get(2)?,
        sort: row.get(3)?,
        url: row.get(4)?,
        auth_type: row.get(5)?,
        auth_params_json: row.get(6)?,
        action: row.get(7)?,
        pool: row.get(8)?,
        wool_max_inflight: row.get(9)?,
        wool_cooldown_until: row.get(10)?,
        wool_failure_count: row.get(11)?,
        wool_last_preflight_at: row.get(12)?,
        fast: row.get(13)?,
        compatibility_mode: row.get(14)?,
        status: row.get(15)?,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
        last_test_at: row.get(18)?,
        last_test_status: row.get(19)?,
        last_test_error: row.get(20)?,
    })
}
