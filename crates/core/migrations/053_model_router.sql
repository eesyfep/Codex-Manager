CREATE TABLE IF NOT EXISTS session_model_memory (
  thread_id TEXT PRIMARY KEY,
  workspace TEXT NOT NULL DEFAULT '',
  title TEXT,
  model TEXT NOT NULL,
  reasoning_effort TEXT,
  source TEXT NOT NULL DEFAULT 'manual',
  locked INTEGER NOT NULL DEFAULT 0,
  last_seen_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_session_model_memory_workspace_updated
  ON session_model_memory(workspace, updated_at DESC);

CREATE TABLE IF NOT EXISTS workspace_model_defaults (
  workspace TEXT PRIMARY KEY,
  default_model TEXT,
  default_reasoning_effort TEXT,
  inherit_last_session INTEGER NOT NULL DEFAULT 1,
  auto_remember INTEGER NOT NULL DEFAULT 1,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_workspace_model_defaults_updated
  ON workspace_model_defaults(updated_at DESC);

CREATE TABLE IF NOT EXISTS model_route_bindings (
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
);

CREATE INDEX IF NOT EXISTS idx_model_route_bindings_model_enabled
  ON model_route_bindings(model, enabled, priority ASC, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_model_route_bindings_api
  ON model_route_bindings(aggregate_api_id);

CREATE TABLE IF NOT EXISTS upstream_model_capabilities (
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
);

CREATE INDEX IF NOT EXISTS idx_upstream_model_capabilities_api_model
  ON upstream_model_capabilities(aggregate_api_id, model);

CREATE TABLE IF NOT EXISTS probe_runs (
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
);

CREATE INDEX IF NOT EXISTS idx_probe_runs_api_started
  ON probe_runs(aggregate_api_id, started_at DESC);

CREATE TABLE IF NOT EXISTS probe_candidates (
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
);

CREATE INDEX IF NOT EXISTS idx_probe_candidates_run
  ON probe_candidates(probe_run_id, created_at ASC);

CREATE INDEX IF NOT EXISTS idx_probe_candidates_model
  ON probe_candidates(model, aggregate_api_id);
