use rusqlite::Result;

use super::{
    ApiKeyTokenUsageSummary, DashboardDailyTokenUsageBucket, DashboardTokenUsageSummary,
    RequestLogTodaySummary, RequestTokenStat, Storage,
};

impl Storage {
    /// 函数 `insert_request_token_stat`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - stat: 参数 stat
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn insert_request_token_stat(&self, stat: &RequestTokenStat) -> Result<()> {
        self.conn.execute(
            "INSERT INTO request_token_stats (
                request_log_id, key_id, account_id, model,
                input_tokens, cached_input_tokens, output_tokens, total_tokens, reasoning_output_tokens,
                estimated_cost_usd, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            (
                stat.request_log_id,
                &stat.key_id,
                &stat.account_id,
                &stat.model,
                stat.input_tokens,
                stat.cached_input_tokens,
                stat.output_tokens,
                stat.total_tokens,
                stat.reasoning_output_tokens,
                stat.estimated_cost_usd,
                stat.created_at,
            ),
        )?;
        Ok(())
    }

    /// 函数 `summarize_request_token_stats_between`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - start_ts: 参数 start_ts
    /// - end_ts: 参数 end_ts
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn summarize_request_token_stats_between(
        &self,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<RequestLogTodaySummary> {
        let mut stmt = self.conn.prepare(
            "SELECT
                IFNULL(SUM(input_tokens), 0),
                IFNULL(SUM(cached_input_tokens), 0),
                IFNULL(SUM(output_tokens), 0),
                IFNULL(SUM(reasoning_output_tokens), 0),
                IFNULL(SUM(estimated_cost_usd), 0.0)
             FROM request_token_stats
             WHERE created_at >= ?1 AND created_at < ?2",
        )?;
        let mut rows = stmt.query((start_ts, end_ts))?;
        if let Some(row) = rows.next()? {
            return Ok(RequestLogTodaySummary {
                input_tokens: row.get(0)?,
                cached_input_tokens: row.get(1)?,
                output_tokens: row.get(2)?,
                reasoning_output_tokens: row.get(3)?,
                estimated_cost_usd: row.get(4)?,
            });
        }
        Ok(RequestLogTodaySummary {
            input_tokens: 0,
            cached_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            estimated_cost_usd: 0.0,
        })
    }

    /// 函数 `summarize_request_token_stats_by_key`
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
    pub fn summarize_request_token_stats_by_key(&self) -> Result<Vec<ApiKeyTokenUsageSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                key_id,
                IFNULL(
                    SUM(
                        CASE
                            WHEN total_tokens IS NOT NULL THEN
                                CASE WHEN total_tokens > 0 THEN total_tokens ELSE 0 END
                            ELSE
                                CASE
                                    WHEN IFNULL(input_tokens, 0) - IFNULL(cached_input_tokens, 0) + IFNULL(output_tokens, 0) > 0
                                        THEN IFNULL(input_tokens, 0) - IFNULL(cached_input_tokens, 0) + IFNULL(output_tokens, 0)
                                    ELSE 0
                                END
                        END
                    ),
                    0
                ) AS total_tokens,
                IFNULL(SUM(estimated_cost_usd), 0.0) AS estimated_cost_usd
             FROM request_token_stats
             WHERE key_id IS NOT NULL AND TRIM(key_id) <> ''
             GROUP BY key_id
             ORDER BY total_tokens DESC, key_id ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(ApiKeyTokenUsageSummary {
                key_id: row.get(0)?,
                total_tokens: row.get(1)?,
                estimated_cost_usd: row.get(2)?,
            });
        }
        Ok(items)
    }

    pub fn summarize_dashboard_token_usage(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        limit: i64,
    ) -> Result<Vec<DashboardTokenUsageSummary>> {
        let limit = limit.clamp(1, 100);
        let mut filters = Vec::new();
        if start_ts.is_some() {
            filters.push("t.created_at >= ?".to_string());
        }
        if end_ts.is_some() {
            filters.push("t.created_at < ?".to_string());
        }
        let where_sql = if filters.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", filters.join(" AND "))
        };
        let sql = format!(
            "SELECT
                t.key_id,
                k.name,
                t.account_id,
                a.label,
                COALESCE(k.aggregate_api_id, r.initial_aggregate_api_id),
                COALESCE(r.aggregate_api_supplier_name, aa.supplier_name),
                COALESCE(r.aggregate_api_url, aa.url),
                t.model,
                COUNT(*) AS request_count,
                IFNULL(SUM(t.input_tokens), 0) AS input_tokens,
                IFNULL(SUM(t.cached_input_tokens), 0) AS cached_input_tokens,
                IFNULL(SUM(t.output_tokens), 0) AS output_tokens,
                IFNULL(SUM(t.reasoning_output_tokens), 0) AS reasoning_output_tokens,
                IFNULL(
                    SUM(
                        CASE
                            WHEN t.total_tokens IS NOT NULL THEN
                                CASE WHEN t.total_tokens > 0 THEN t.total_tokens ELSE 0 END
                            ELSE
                                CASE
                                    WHEN IFNULL(t.input_tokens, 0) - IFNULL(t.cached_input_tokens, 0) + IFNULL(t.output_tokens, 0) > 0
                                        THEN IFNULL(t.input_tokens, 0) - IFNULL(t.cached_input_tokens, 0) + IFNULL(t.output_tokens, 0)
                                    ELSE 0
                                END
                        END
                    ),
                    0
                ) AS total_tokens,
                IFNULL(SUM(t.estimated_cost_usd), 0.0) AS estimated_cost_usd,
                MAX(t.created_at) AS last_used_at
             FROM request_token_stats t
             LEFT JOIN request_logs r ON r.id = t.request_log_id
             LEFT JOIN api_keys k ON k.id = t.key_id
             LEFT JOIN aggregate_apis aa ON aa.id = COALESCE(k.aggregate_api_id, r.initial_aggregate_api_id)
             LEFT JOIN accounts a ON a.id = t.account_id
             {where_sql}
             GROUP BY
                COALESCE(t.key_id, ''),
                COALESCE(t.account_id, ''),
                COALESCE(k.aggregate_api_id, r.initial_aggregate_api_id, ''),
                COALESCE(r.aggregate_api_url, aa.url, ''),
                COALESCE(t.model, '')
             ORDER BY total_tokens DESC, estimated_cost_usd DESC, last_used_at DESC
             LIMIT ?"
        );
        let mut params = Vec::new();
        if let Some(value) = start_ts {
            params.push(rusqlite::types::Value::Integer(value));
        }
        if let Some(value) = end_ts {
            params.push(rusqlite::types::Value::Integer(value));
        }
        params.push(rusqlite::types::Value::Integer(limit));

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok(DashboardTokenUsageSummary {
                key_id: row.get(0)?,
                key_name: row.get(1)?,
                account_id: row.get(2)?,
                account_label: row.get(3)?,
                aggregate_api_id: row.get(4)?,
                aggregate_api_supplier_name: row.get(5)?,
                aggregate_api_url: row.get(6)?,
                model: row.get(7)?,
                request_count: row.get(8)?,
                input_tokens: row.get(9)?,
                cached_input_tokens: row.get(10)?,
                output_tokens: row.get(11)?,
                reasoning_output_tokens: row.get(12)?,
                total_tokens: row.get(13)?,
                estimated_cost_usd: row.get(14)?,
                last_used_at: row.get(15)?,
            })
        })?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    pub fn summarize_dashboard_daily_token_usage(
        &self,
        start_ts: i64,
        end_ts: i64,
        limit_per_day: i64,
    ) -> Result<Vec<DashboardDailyTokenUsageBucket>> {
        let limit_per_day = limit_per_day.clamp(1, 64);
        let mut stmt = self.conn.prepare(
            "WITH grouped AS (
                SELECT
                    ((t.created_at + CAST(strftime('%s', 'now', 'localtime') AS INTEGER) - CAST(strftime('%s', 'now') AS INTEGER)) / 86400) * 86400
                        - (CAST(strftime('%s', 'now', 'localtime') AS INTEGER) - CAST(strftime('%s', 'now') AS INTEGER)) AS day_start_ts,
                    COALESCE(
                        NULLIF(TRIM(COALESCE(r.aggregate_api_url, aa.url)), ''),
                        NULLIF(TRIM(t.model), ''),
                        'unknown'
                    ) AS source_key,
                    COALESCE(
                        NULLIF(TRIM(COALESCE(r.aggregate_api_supplier_name, aa.supplier_name)), ''),
                        NULLIF(TRIM(COALESCE(r.aggregate_api_url, aa.url)), ''),
                        NULLIF(TRIM(t.model), ''),
                        '未知模型'
                    ) AS source_label,
                    NULLIF(TRIM(t.model), '') AS model,
                    COUNT(*) AS request_count,
                    IFNULL(SUM(t.input_tokens), 0) AS input_tokens,
                    IFNULL(SUM(t.cached_input_tokens), 0) AS cached_input_tokens,
                    IFNULL(
                        SUM(
                            CASE
                                WHEN IFNULL(t.input_tokens, 0) - IFNULL(t.cached_input_tokens, 0) > 0
                                    THEN IFNULL(t.input_tokens, 0) - IFNULL(t.cached_input_tokens, 0)
                                ELSE 0
                            END
                        ),
                        0
                    ) AS billable_input_tokens,
                    IFNULL(SUM(t.output_tokens), 0) AS output_tokens,
                    IFNULL(SUM(t.reasoning_output_tokens), 0) AS reasoning_output_tokens,
                    IFNULL(
                        SUM(
                            CASE
                                WHEN t.total_tokens IS NOT NULL THEN
                                    CASE WHEN t.total_tokens > 0 THEN t.total_tokens ELSE 0 END
                                ELSE
                                    CASE
                                        WHEN IFNULL(t.input_tokens, 0) - IFNULL(t.cached_input_tokens, 0) + IFNULL(t.output_tokens, 0) > 0
                                            THEN IFNULL(t.input_tokens, 0) - IFNULL(t.cached_input_tokens, 0) + IFNULL(t.output_tokens, 0)
                                        ELSE 0
                                    END
                            END
                        ),
                        0
                    ) AS total_tokens,
                    IFNULL(SUM(t.estimated_cost_usd), 0.0) AS estimated_cost_usd
                 FROM request_token_stats t
                 LEFT JOIN request_logs r ON r.id = t.request_log_id
                 LEFT JOIN api_keys k ON k.id = t.key_id
                 LEFT JOIN aggregate_apis aa ON aa.id = COALESCE(k.aggregate_api_id, r.initial_aggregate_api_id)
                 WHERE t.created_at >= ?1 AND t.created_at < ?2
                 GROUP BY
                    day_start_ts,
                    COALESCE(
                        NULLIF(TRIM(COALESCE(r.aggregate_api_url, aa.url)), ''),
                        NULLIF(TRIM(t.model), ''),
                        'unknown'
                    ),
                    NULLIF(TRIM(t.model), '')
             ),
             ranked AS (
                 SELECT
                    grouped.*,
                    ROW_NUMBER() OVER (
                        PARTITION BY day_start_ts
                        ORDER BY total_tokens DESC, estimated_cost_usd DESC, request_count DESC
                    ) AS source_rank
                 FROM grouped
             )
             SELECT
                day_start_ts,
                CASE WHEN source_rank <= ?3 THEN source_key ELSE '__other__' END AS bucket_key,
                CASE WHEN source_rank <= ?3 THEN source_label ELSE '其他来源' END AS bucket_label,
                CASE WHEN source_rank <= ?3 THEN model ELSE NULL END AS bucket_model,
                SUM(request_count) AS request_count,
                SUM(input_tokens) AS input_tokens,
                SUM(cached_input_tokens) AS cached_input_tokens,
                SUM(billable_input_tokens) AS billable_input_tokens,
                SUM(output_tokens) AS output_tokens,
                SUM(reasoning_output_tokens) AS reasoning_output_tokens,
                SUM(total_tokens) AS total_tokens,
                SUM(estimated_cost_usd) AS estimated_cost_usd
             FROM ranked
             GROUP BY day_start_ts, bucket_key, bucket_label, bucket_model
             ORDER BY day_start_ts ASC, total_tokens DESC",
        )?;
        let rows = stmt.query_map((start_ts, end_ts, limit_per_day), |row| {
            Ok(DashboardDailyTokenUsageBucket {
                day_start_ts: row.get(0)?,
                source_key: row.get(1)?,
                source_label: row.get(2)?,
                model: row.get(3)?,
                request_count: row.get(4)?,
                input_tokens: row.get(5)?,
                cached_input_tokens: row.get(6)?,
                billable_input_tokens: row.get(7)?,
                output_tokens: row.get(8)?,
                reasoning_output_tokens: row.get(9)?,
                total_tokens: row.get(10)?,
                estimated_cost_usd: row.get(11)?,
            })
        })?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    /// 函数 `ensure_request_token_stats_table`
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
    pub(super) fn ensure_request_token_stats_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS request_token_stats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                request_log_id INTEGER NOT NULL,
                key_id TEXT,
                account_id TEXT,
                model TEXT,
                input_tokens INTEGER,
                cached_input_tokens INTEGER,
                output_tokens INTEGER,
                total_tokens INTEGER,
                reasoning_output_tokens INTEGER,
                estimated_cost_usd REAL,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_request_token_stats_request_log_id
             ON request_token_stats(request_log_id)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stats_created_at
             ON request_token_stats(created_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stats_account_id_created_at
             ON request_token_stats(account_id, created_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stats_key_id_created_at
             ON request_token_stats(key_id, created_at DESC)",
            [],
        )?;
        self.ensure_column("request_token_stats", "total_tokens", "INTEGER")?;

        if self.has_column("request_logs", "input_tokens")? {
            // 中文注释：迁移历史 request_logs 里的 token 字段，避免升级后今日统计突然归零。
            self.conn.execute(
                "INSERT OR IGNORE INTO request_token_stats (
                    request_log_id, key_id, account_id, model,
                    input_tokens, cached_input_tokens, output_tokens, total_tokens, reasoning_output_tokens,
                    estimated_cost_usd, created_at
                 )
                 SELECT
                    id, key_id, account_id, model,
                    input_tokens, cached_input_tokens, output_tokens, NULL, reasoning_output_tokens,
                    estimated_cost_usd, created_at
                 FROM request_logs
                 WHERE input_tokens IS NOT NULL
                    OR cached_input_tokens IS NOT NULL
                    OR output_tokens IS NOT NULL
                    OR reasoning_output_tokens IS NOT NULL
                    OR estimated_cost_usd IS NOT NULL",
                [],
            )?;
        }
        Ok(())
    }
}
