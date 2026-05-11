export interface SessionModelSummary {
  threadId: string;
  workspace: string;
  title: string | null;
  model: string | null;
  reasoningEffort: string | null;
  modelProvider: string | null;
  effectiveModelLabel: string;
  effectiveModelSource: string;
  hasModelOverride: boolean;
  parentThreadId: string | null;
  isSubagent: boolean;
  agentNickname: string | null;
  agentRole: string | null;
  subagentDepth: number | null;
  source: string;
  locked: boolean;
  memoryState: string;
  lastSeenAt: number;
  updatedAt: number;
  subagentModel: string | null;
  subagentReasoningEffort: string | null;
  subagentModelSource: string | null;
  subagentModelUpdatedAt: number | null;
}

export interface WorkspaceModelDefaultSummary {
  workspace: string;
  defaultModel: string | null;
  defaultReasoningEffort: string | null;
  inheritLastSession: boolean;
  autoRemember: boolean;
  updatedAt: number;
}

export interface SessionModelListResult {
  items: SessionModelSummary[];
  workspaceDefaults: WorkspaceModelDefaultSummary[];
  stateDbPath: string | null;
  stateDbOk: boolean;
  stateDbError: string | null;
  globalDefaultModel: string | null;
}

export interface SessionModelUpdateResult {
  item: SessionModelSummary;
  stateUpdated: boolean;
}

export interface LatestSessionModelApplyResult {
  item: SessionModelSummary;
  stateUpdated: boolean;
  matchedWorkspace: string;
}

export interface ModelRouteBindingSummary {
  id: string;
  model: string;
  aggregateApiId: string;
  aggregateApiName: string | null;
  aggregateApiUrl: string | null;
  enabled: boolean;
  priority: number;
  weight: number;
  routeStrategy: string;
  manualPreferred: boolean;
  supportsResponses: boolean;
  supportsChatCompletions: boolean;
  requiresAdapter: boolean;
  lastProbeStatus: string | null;
  lastError: string | null;
  lastSuccessAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface ModelRouteBindingListResult {
  items: ModelRouteBindingSummary[];
}

export interface ModelRouteBindingSavePayload {
  id?: string | null;
  model: string;
  aggregateApiId: string;
  enabled?: boolean;
  priority?: number;
  weight?: number;
  routeStrategy?: string;
  manualPreferred?: boolean;
  supportsResponses?: boolean;
  supportsChatCompletions?: boolean;
  requiresAdapter?: boolean;
}

export interface ModelRouteBindingSaveResult {
  item: ModelRouteBindingSummary;
}

export interface ProbeCandidateSummary {
  id: string;
  probeRunId: string;
  aggregateApiId: string;
  model: string;
  supportsResponses: boolean;
  supportsChatCompletions: boolean;
  requiresAdapter: boolean;
  suggestedRouteStrategy: string;
  suggestedPriority: number;
  suggestedWeight: number;
  applied: boolean;
  error: string | null;
  createdAt: number;
  appliedAt: number | null;
}

export interface ProbeRunSummary {
  id: string;
  aggregateApiId: string;
  aggregateApiName: string | null;
  status: string;
  startedAt: number;
  finishedAt: number | null;
  modelsStatus: string | null;
  responsesStatus: string | null;
  chatCompletionsStatus: string | null;
  error: string | null;
  candidates: ProbeCandidateSummary[];
  rawSummary: Record<string, unknown> | null;
}

export interface ProbeApplySelectedPayload {
  probeRunId: string;
  candidateIds: string[];
}

export interface ProbeRunListResult {
  items: ProbeRunSummary[];
}

export interface ProbeRunAllResult {
  items: ProbeRunSummary[];
  attempted: number;
  succeeded: number;
  failed: number;
}

export interface ModelRouteQuickCheckResult {
  aggregateApiId: string;
  aggregateApiName: string | null;
  model: string;
  ok: boolean;
  statusCode: number | null;
  protocol: string;
  responseAdapter: string | null;
  latencyMs: number;
  error: string | null;
  checkedAt: number;
}

export interface ModelRouterImportResult {
  sourcePath: string;
  backupPath: string | null;
  aggregateApis: number;
  aggregateApiSecrets: number;
  apiKeys: number;
  apiKeySecrets: number;
  routeBindings: number;
  workspaceDefaults: number;
  appSettings: number;
}
