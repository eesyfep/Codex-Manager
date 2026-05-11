use rusqlite::{params, Result, Row};

use super::{
    now_ts, ModelRouteBinding, ProbeCandidate, ProbeRun, SessionModelMemory,
    SessionSubagentModelMemory, Storage, UpstreamModelCapability, WorkspaceModelDefault,
};

const MODEL_ROUTE_BINDING_SELECT_SQL: &str = "SELECT
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
    last_probe_status,
    last_error,
    last_success_at,
    created_at,
    updated_at
 FROM model_route_bindings";

const PROBE_CANDIDATE_SELECT_SQL: &str = "SELECT
    id,
    probe_run_id,
    aggregate_api_id,
    model,
    supports_responses,
    supports_chat_completions,
    requires_adapter,
    suggested_route_strategy,
    suggested_priority,
    suggested_weight,
    applied,
    error,
    created_at,
    applied_at
 FROM probe_candidates";

const UPSTREAM_MODEL_CAPABILITY_SELECT_SQL: &str = "SELECT
    id,
    aggregate_api_id,
    model,
    supports_responses,
    supports_chat_completions,
    requires_adapter,
    probe_status,
    last_error,
    last_probe_at,
    updated_at
 FROM upstream_model_capabilities";

impl Storage {
    pub fn upsert_session_model_memory(&self, item: &SessionModelMemory) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_model_memory (
                thread_id, workspace, title, model, reasoning_effort, source, locked, last_seen_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(thread_id) DO UPDATE SET
                workspace = excluded.workspace,
                title = excluded.title,
                model = excluded.model,
                reasoning_effort = excluded.reasoning_effort,
                source = excluded.source,
                locked = excluded.locked,
                last_seen_at = excluded.last_seen_at,
                updated_at = excluded.updated_at",
            params![
                &item.thread_id,
                &item.workspace,
                &item.title,
                &item.model,
                &item.reasoning_effort,
                &item.source,
                item.locked,
                item.last_seen_at,
                item.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn find_session_model_memory(&self, thread_id: &str) -> Result<Option<SessionModelMemory>> {
        let mut stmt = self.conn.prepare(
            "SELECT thread_id, workspace, title, model, reasoning_effort, source, locked, last_seen_at, updated_at
             FROM session_model_memory
             WHERE thread_id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([thread_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_session_model_memory_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_session_model_memory(
        &self,
        workspace: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SessionModelMemory>> {
        let normalized_limit = if limit <= 0 { 200 } else { limit.min(1000) };
        let mut out = Vec::new();
        if let Some(workspace) = workspace.map(str::trim).filter(|value| !value.is_empty()) {
            let mut stmt = self.conn.prepare(
                "SELECT thread_id, workspace, title, model, reasoning_effort, source, locked, last_seen_at, updated_at
                 FROM session_model_memory
                 WHERE workspace = ?1
                 ORDER BY updated_at DESC
                 LIMIT ?2",
            )?;
            let mut rows = stmt.query(params![workspace, normalized_limit])?;
            while let Some(row) = rows.next()? {
                out.push(map_session_model_memory_row(row)?);
            }
            return Ok(out);
        }

        let mut stmt = self.conn.prepare(
            "SELECT thread_id, workspace, title, model, reasoning_effort, source, locked, last_seen_at, updated_at
             FROM session_model_memory
             ORDER BY updated_at DESC
             LIMIT ?1",
        )?;
        let mut rows = stmt.query([normalized_limit])?;
        while let Some(row) = rows.next()? {
            out.push(map_session_model_memory_row(row)?);
        }
        Ok(out)
    }

    pub fn latest_workspace_session_model(
        &self,
        workspace: &str,
    ) -> Result<Option<SessionModelMemory>> {
        let mut stmt = self.conn.prepare(
            "SELECT thread_id, workspace, title, model, reasoning_effort, source, locked, last_seen_at, updated_at
             FROM session_model_memory
             WHERE workspace = ?1
               AND TRIM(model) <> ''
               AND source NOT IN ('state', 'parent_subagent_default')
             ORDER BY updated_at DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query([workspace])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_session_model_memory_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_session_model_lock(&self, thread_id: &str, locked: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE session_model_memory SET locked = ?1, updated_at = ?2 WHERE thread_id = ?3",
            params![locked, now_ts(), thread_id],
        )?;
        Ok(())
    }

    pub fn upsert_session_subagent_model_memory(
        &self,
        item: &SessionSubagentModelMemory,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_subagent_model_memory (
                parent_thread_id, workspace, model, reasoning_effort, source, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(parent_thread_id) DO UPDATE SET
                workspace = excluded.workspace,
                model = excluded.model,
                reasoning_effort = excluded.reasoning_effort,
                source = excluded.source,
                updated_at = excluded.updated_at",
            params![
                &item.parent_thread_id,
                &item.workspace,
                &item.model,
                &item.reasoning_effort,
                &item.source,
                item.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn find_session_subagent_model_memory(
        &self,
        parent_thread_id: &str,
    ) -> Result<Option<SessionSubagentModelMemory>> {
        let mut stmt = self.conn.prepare(
            "SELECT parent_thread_id, workspace, model, reasoning_effort, source, updated_at
             FROM session_subagent_model_memory
             WHERE parent_thread_id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([parent_thread_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_session_subagent_model_memory_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn delete_session_subagent_model_memory(&self, parent_thread_id: &str) -> Result<bool> {
        let deleted = self.conn.execute(
            "DELETE FROM session_subagent_model_memory WHERE parent_thread_id = ?1",
            [parent_thread_id],
        )?;
        Ok(deleted > 0)
    }

    pub fn delete_inherited_subagent_session_model_memory(
        &self,
        thread_ids: &[String],
    ) -> Result<usize> {
        let mut deleted = 0usize;
        for thread_id in thread_ids {
            deleted += self.conn.execute(
                "DELETE FROM session_model_memory
                 WHERE thread_id = ?1 AND source = 'parent_subagent_default'",
                [thread_id],
            )?;
        }
        Ok(deleted)
    }

    pub fn upsert_workspace_model_default(&self, item: &WorkspaceModelDefault) -> Result<()> {
        self.conn.execute(
            "INSERT INTO workspace_model_defaults (
                workspace, default_model, default_reasoning_effort, inherit_last_session, auto_remember, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(workspace) DO UPDATE SET
                default_model = excluded.default_model,
                default_reasoning_effort = excluded.default_reasoning_effort,
                inherit_last_session = excluded.inherit_last_session,
                auto_remember = excluded.auto_remember,
                updated_at = excluded.updated_at",
            params![
                &item.workspace,
                &item.default_model,
                &item.default_reasoning_effort,
                item.inherit_last_session,
                item.auto_remember,
                item.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn find_workspace_model_default(
        &self,
        workspace: &str,
    ) -> Result<Option<WorkspaceModelDefault>> {
        let mut stmt = self.conn.prepare(
            "SELECT workspace, default_model, default_reasoning_effort, inherit_last_session, auto_remember, updated_at
             FROM workspace_model_defaults
             WHERE workspace = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([workspace])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_workspace_model_default_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_workspace_model_defaults(&self) -> Result<Vec<WorkspaceModelDefault>> {
        let mut stmt = self.conn.prepare(
            "SELECT workspace, default_model, default_reasoning_effort, inherit_last_session, auto_remember, updated_at
             FROM workspace_model_defaults
             ORDER BY updated_at DESC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_workspace_model_default_row(row)?);
        }
        Ok(out)
    }

    pub fn delete_workspace_model_default(&self, workspace: &str) -> Result<bool> {
        let deleted = self.conn.execute(
            "DELETE FROM workspace_model_defaults WHERE workspace = ?1",
            [workspace],
        )?;
        Ok(deleted > 0)
    }

    pub fn upsert_model_route_binding(&self, item: &ModelRouteBinding) -> Result<()> {
        self.conn.execute(
            "INSERT INTO model_route_bindings (
                id, model, aggregate_api_id, enabled, priority, weight, route_strategy, manual_preferred,
                supports_responses, supports_chat_completions, requires_adapter,
                last_probe_status, last_error, last_success_at, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
             ON CONFLICT(model, aggregate_api_id) DO UPDATE SET
                enabled = excluded.enabled,
                priority = excluded.priority,
                weight = excluded.weight,
                route_strategy = excluded.route_strategy,
                manual_preferred = excluded.manual_preferred,
                supports_responses = excluded.supports_responses,
                supports_chat_completions = excluded.supports_chat_completions,
                requires_adapter = excluded.requires_adapter,
                last_probe_status = excluded.last_probe_status,
                last_error = excluded.last_error,
                last_success_at = excluded.last_success_at,
                updated_at = excluded.updated_at",
            params![
                &item.id,
                &item.model,
                &item.aggregate_api_id,
                item.enabled,
                item.priority,
                item.weight,
                &item.route_strategy,
                item.manual_preferred,
                item.supports_responses,
                item.supports_chat_completions,
                item.requires_adapter,
                &item.last_probe_status,
                &item.last_error,
                &item.last_success_at,
                item.created_at,
                item.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_model_route_bindings(&self, model: Option<&str>) -> Result<Vec<ModelRouteBinding>> {
        let mut out = Vec::new();
        if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
            let mut stmt = self.conn.prepare(&format!(
                "{MODEL_ROUTE_BINDING_SELECT_SQL}
                 WHERE model = ?1
                 ORDER BY manual_preferred DESC, priority ASC, weight DESC, updated_at DESC"
            ))?;
            let mut rows = stmt.query([model])?;
            while let Some(row) = rows.next()? {
                out.push(map_model_route_binding_row(row)?);
            }
            return Ok(out);
        }

        let mut stmt = self.conn.prepare(&format!(
            "{MODEL_ROUTE_BINDING_SELECT_SQL}
             ORDER BY model ASC, manual_preferred DESC, priority ASC, weight DESC, updated_at DESC"
        ))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            out.push(map_model_route_binding_row(row)?);
        }
        Ok(out)
    }

    pub fn list_enabled_model_route_bindings(&self, model: &str) -> Result<Vec<ModelRouteBinding>> {
        let mut stmt = self.conn.prepare(&format!(
            "{MODEL_ROUTE_BINDING_SELECT_SQL}
             WHERE model = ?1 AND enabled = 1
             ORDER BY manual_preferred DESC, priority ASC, weight DESC, updated_at DESC"
        ))?;
        let mut rows = stmt.query([model])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_model_route_binding_row(row)?);
        }
        Ok(out)
    }

    pub fn delete_model_route_binding(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM model_route_bindings WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn clear_manual_preferred_model_route_bindings(
        &self,
        model: &str,
        except_id: Option<&str>,
    ) -> Result<usize> {
        match except_id.map(str::trim).filter(|value| !value.is_empty()) {
            Some(except_id) => self.conn.execute(
                "UPDATE model_route_bindings
                 SET manual_preferred = 0, updated_at = ?1
                 WHERE model = ?2 AND id <> ?3",
                params![now_ts(), model, except_id],
            ),
            None => self.conn.execute(
                "UPDATE model_route_bindings
                 SET manual_preferred = 0, updated_at = ?1
                 WHERE model = ?2",
                params![now_ts(), model],
            ),
        }
    }

    pub fn update_model_route_binding_result(
        &self,
        model: &str,
        aggregate_api_id: &str,
        status: &str,
        last_error: Option<&str>,
        last_success_at: Option<i64>,
    ) -> Result<usize> {
        self.conn.execute(
            "UPDATE model_route_bindings
             SET last_probe_status = ?1,
                 last_error = ?2,
                 last_success_at = COALESCE(?3, last_success_at),
                 updated_at = ?4
             WHERE model = ?5 AND aggregate_api_id = ?6 AND enabled = 1",
            params![
                status,
                last_error,
                last_success_at,
                now_ts(),
                model,
                aggregate_api_id,
            ],
        )
    }

    pub fn update_model_route_binding_probe_result(
        &self,
        model: &str,
        aggregate_api_id: &str,
        status: &str,
        last_error: Option<&str>,
        last_success_at: Option<i64>,
        supports_responses: Option<bool>,
        supports_chat_completions: Option<bool>,
        requires_adapter: Option<bool>,
    ) -> Result<usize> {
        self.conn.execute(
            "UPDATE model_route_bindings
             SET last_probe_status = ?1,
                 last_error = ?2,
                 last_success_at = COALESCE(?3, last_success_at),
                 supports_responses = COALESCE(?4, supports_responses),
                 supports_chat_completions = COALESCE(?5, supports_chat_completions),
                 requires_adapter = COALESCE(?6, requires_adapter),
                 updated_at = ?7
             WHERE model = ?8 AND aggregate_api_id = ?9 AND enabled = 1",
            params![
                status,
                last_error,
                last_success_at,
                supports_responses,
                supports_chat_completions,
                requires_adapter,
                now_ts(),
                model,
                aggregate_api_id,
            ],
        )
    }

    pub fn upsert_upstream_model_capability(&self, item: &UpstreamModelCapability) -> Result<()> {
        self.conn.execute(
            "INSERT INTO upstream_model_capabilities (
                id, aggregate_api_id, model, supports_responses, supports_chat_completions,
                requires_adapter, probe_status, last_error, last_probe_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(aggregate_api_id, model) DO UPDATE SET
                supports_responses = excluded.supports_responses,
                supports_chat_completions = excluded.supports_chat_completions,
                requires_adapter = excluded.requires_adapter,
                probe_status = excluded.probe_status,
                last_error = excluded.last_error,
                last_probe_at = excluded.last_probe_at,
                updated_at = excluded.updated_at",
            params![
                &item.id,
                &item.aggregate_api_id,
                &item.model,
                item.supports_responses,
                item.supports_chat_completions,
                item.requires_adapter,
                &item.probe_status,
                &item.last_error,
                &item.last_probe_at,
                item.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_upstream_model_capabilities(
        &self,
        model: Option<&str>,
    ) -> Result<Vec<UpstreamModelCapability>> {
        let mut out = Vec::new();
        if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
            let mut stmt = self.conn.prepare(&format!(
                "{UPSTREAM_MODEL_CAPABILITY_SELECT_SQL}
                 WHERE model = ?1
                 ORDER BY updated_at DESC, aggregate_api_id ASC"
            ))?;
            let mut rows = stmt.query([model])?;
            while let Some(row) = rows.next()? {
                out.push(map_upstream_model_capability_row(row)?);
            }
            return Ok(out);
        }

        let mut stmt = self.conn.prepare(&format!(
            "{UPSTREAM_MODEL_CAPABILITY_SELECT_SQL}
             ORDER BY updated_at DESC, aggregate_api_id ASC"
        ))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            out.push(map_upstream_model_capability_row(row)?);
        }
        Ok(out)
    }

    pub fn insert_probe_run(&self, item: &ProbeRun) -> Result<()> {
        self.conn.execute(
            "INSERT INTO probe_runs (
                id, aggregate_api_id, status, started_at, finished_at, models_status,
                responses_status, chat_completions_status, error, raw_summary_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                &item.id,
                &item.aggregate_api_id,
                &item.status,
                item.started_at,
                &item.finished_at,
                &item.models_status,
                &item.responses_status,
                &item.chat_completions_status,
                &item.error,
                &item.raw_summary_json,
            ],
        )?;
        Ok(())
    }

    pub fn insert_probe_candidate(&self, item: &ProbeCandidate) -> Result<()> {
        self.conn.execute(
            "INSERT INTO probe_candidates (
                id, probe_run_id, aggregate_api_id, model, supports_responses,
                supports_chat_completions, requires_adapter, suggested_route_strategy,
                suggested_priority, suggested_weight, applied, error, created_at, applied_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                &item.id,
                &item.probe_run_id,
                &item.aggregate_api_id,
                &item.model,
                item.supports_responses,
                item.supports_chat_completions,
                item.requires_adapter,
                &item.suggested_route_strategy,
                item.suggested_priority,
                item.suggested_weight,
                item.applied,
                &item.error,
                item.created_at,
                &item.applied_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_probe_candidates(&self, probe_run_id: &str) -> Result<Vec<ProbeCandidate>> {
        let mut stmt = self.conn.prepare(&format!(
            "{PROBE_CANDIDATE_SELECT_SQL}
             WHERE probe_run_id = ?1
             ORDER BY created_at ASC, model ASC"
        ))?;
        let mut rows = stmt.query([probe_run_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_probe_candidate_row(row)?);
        }
        Ok(out)
    }

    pub fn latest_probe_runs(&self, limit: i64) -> Result<Vec<ProbeRun>> {
        let normalized_limit = if limit <= 0 { 20 } else { limit.min(200) };
        let mut stmt = self.conn.prepare(
            "SELECT id, aggregate_api_id, status, started_at, finished_at, models_status,
                    responses_status, chat_completions_status, error, raw_summary_json
             FROM probe_runs
             ORDER BY started_at DESC
             LIMIT ?1",
        )?;
        let mut rows = stmt.query([normalized_limit])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_probe_run_row(row)?);
        }
        Ok(out)
    }

    pub fn mark_probe_candidates_applied(&self, probe_run_id: &str, applied_at: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE probe_candidates SET applied = 1, applied_at = ?1 WHERE probe_run_id = ?2",
            params![applied_at, probe_run_id],
        )?;
        Ok(())
    }

    pub(super) fn ensure_model_router_tables(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS session_model_memory (
                thread_id TEXT PRIMARY KEY,
                workspace TEXT NOT NULL DEFAULT '',
                title TEXT,
                model TEXT NOT NULL,
                reasoning_effort TEXT,
                source TEXT NOT NULL DEFAULT 'manual',
                locked INTEGER NOT NULL DEFAULT 0,
                last_seen_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_model_memory_workspace_updated
             ON session_model_memory(workspace, updated_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS session_subagent_model_memory (
                parent_thread_id TEXT PRIMARY KEY,
                workspace TEXT NOT NULL DEFAULT '',
                model TEXT NOT NULL,
                reasoning_effort TEXT,
                source TEXT NOT NULL DEFAULT 'manual',
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_subagent_model_memory_workspace_updated
             ON session_subagent_model_memory(workspace, updated_at DESC)",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS workspace_model_defaults (
                workspace TEXT PRIMARY KEY,
                default_model TEXT,
                default_reasoning_effort TEXT,
                inherit_last_session INTEGER NOT NULL DEFAULT 1,
                auto_remember INTEGER NOT NULL DEFAULT 1,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS model_route_bindings (
                id TEXT PRIMARY KEY,
                model TEXT NOT NULL,
                aggregate_api_id TEXT NOT NULL REFERENCES aggregate_apis(id) ON DELETE CASCADE,
                enabled INTEGER NOT NULL DEFAULT 1,
                priority INTEGER NOT NULL DEFAULT 0,
                weight INTEGER NOT NULL DEFAULT 1,
                route_strategy TEXT NOT NULL DEFAULT 'ordered',
                manual_preferred INTEGER NOT NULL DEFAULT 0,
                supports_responses INTEGER NOT NULL DEFAULT 0,
                supports_chat_completions INTEGER NOT NULL DEFAULT 0,
                requires_adapter INTEGER NOT NULL DEFAULT 0,
                last_probe_status TEXT,
                last_error TEXT,
                last_success_at INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                UNIQUE(model, aggregate_api_id)
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_model_route_bindings_model_enabled
             ON model_route_bindings(model, enabled, priority ASC, updated_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_model_route_bindings_api
             ON model_route_bindings(aggregate_api_id)",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS upstream_model_capabilities (
                id TEXT PRIMARY KEY,
                aggregate_api_id TEXT NOT NULL REFERENCES aggregate_apis(id) ON DELETE CASCADE,
                model TEXT NOT NULL,
                supports_responses INTEGER NOT NULL DEFAULT 0,
                supports_chat_completions INTEGER NOT NULL DEFAULT 0,
                requires_adapter INTEGER NOT NULL DEFAULT 0,
                probe_status TEXT NOT NULL DEFAULT 'unknown',
                last_error TEXT,
                last_probe_at INTEGER,
                updated_at INTEGER NOT NULL,
                UNIQUE(aggregate_api_id, model)
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS probe_runs (
                id TEXT PRIMARY KEY,
                aggregate_api_id TEXT NOT NULL REFERENCES aggregate_apis(id) ON DELETE CASCADE,
                status TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                models_status TEXT,
                responses_status TEXT,
                chat_completions_status TEXT,
                error TEXT,
                raw_summary_json TEXT
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS probe_candidates (
                id TEXT PRIMARY KEY,
                probe_run_id TEXT NOT NULL REFERENCES probe_runs(id) ON DELETE CASCADE,
                aggregate_api_id TEXT NOT NULL REFERENCES aggregate_apis(id) ON DELETE CASCADE,
                model TEXT NOT NULL,
                supports_responses INTEGER NOT NULL DEFAULT 0,
                supports_chat_completions INTEGER NOT NULL DEFAULT 0,
                requires_adapter INTEGER NOT NULL DEFAULT 0,
                suggested_route_strategy TEXT NOT NULL DEFAULT 'ordered',
                suggested_priority INTEGER NOT NULL DEFAULT 0,
                suggested_weight INTEGER NOT NULL DEFAULT 1,
                applied INTEGER NOT NULL DEFAULT 0,
                error TEXT,
                created_at INTEGER NOT NULL,
                applied_at INTEGER
            )",
            [],
        )?;
        Ok(())
    }
}

fn map_session_model_memory_row(row: &Row<'_>) -> Result<SessionModelMemory> {
    Ok(SessionModelMemory {
        thread_id: row.get(0)?,
        workspace: row.get(1)?,
        title: row.get(2)?,
        model: row.get(3)?,
        reasoning_effort: row.get(4)?,
        source: row.get(5)?,
        locked: row.get(6)?,
        last_seen_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn map_session_subagent_model_memory_row(row: &Row<'_>) -> Result<SessionSubagentModelMemory> {
    Ok(SessionSubagentModelMemory {
        parent_thread_id: row.get(0)?,
        workspace: row.get(1)?,
        model: row.get(2)?,
        reasoning_effort: row.get(3)?,
        source: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn map_workspace_model_default_row(row: &Row<'_>) -> Result<WorkspaceModelDefault> {
    Ok(WorkspaceModelDefault {
        workspace: row.get(0)?,
        default_model: row.get(1)?,
        default_reasoning_effort: row.get(2)?,
        inherit_last_session: row.get(3)?,
        auto_remember: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn map_model_route_binding_row(row: &Row<'_>) -> Result<ModelRouteBinding> {
    Ok(ModelRouteBinding {
        id: row.get(0)?,
        model: row.get(1)?,
        aggregate_api_id: row.get(2)?,
        enabled: row.get(3)?,
        priority: row.get(4)?,
        weight: row.get(5)?,
        route_strategy: row.get(6)?,
        manual_preferred: row.get(7)?,
        supports_responses: row.get(8)?,
        supports_chat_completions: row.get(9)?,
        requires_adapter: row.get(10)?,
        last_probe_status: row.get(11)?,
        last_error: row.get(12)?,
        last_success_at: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

fn map_probe_run_row(row: &Row<'_>) -> Result<ProbeRun> {
    Ok(ProbeRun {
        id: row.get(0)?,
        aggregate_api_id: row.get(1)?,
        status: row.get(2)?,
        started_at: row.get(3)?,
        finished_at: row.get(4)?,
        models_status: row.get(5)?,
        responses_status: row.get(6)?,
        chat_completions_status: row.get(7)?,
        error: row.get(8)?,
        raw_summary_json: row.get(9)?,
    })
}

fn map_probe_candidate_row(row: &Row<'_>) -> Result<ProbeCandidate> {
    Ok(ProbeCandidate {
        id: row.get(0)?,
        probe_run_id: row.get(1)?,
        aggregate_api_id: row.get(2)?,
        model: row.get(3)?,
        supports_responses: row.get(4)?,
        supports_chat_completions: row.get(5)?,
        requires_adapter: row.get(6)?,
        suggested_route_strategy: row.get(7)?,
        suggested_priority: row.get(8)?,
        suggested_weight: row.get(9)?,
        applied: row.get(10)?,
        error: row.get(11)?,
        created_at: row.get(12)?,
        applied_at: row.get(13)?,
    })
}

fn map_upstream_model_capability_row(row: &Row<'_>) -> Result<UpstreamModelCapability> {
    Ok(UpstreamModelCapability {
        id: row.get(0)?,
        aggregate_api_id: row.get(1)?,
        model: row.get(2)?,
        supports_responses: row.get(3)?,
        supports_chat_completions: row.get(4)?,
        requires_adapter: row.get(5)?,
        probe_status: row.get(6)?,
        last_error: row.get(7)?,
        last_probe_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}
