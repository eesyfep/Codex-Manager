export interface ApiKey {
  id: string;
  name: string;
  model: string;
  modelSlug: string;
  reasoningEffort: string;
  serviceTier: string;
  rotationStrategy: string;
  aggregateApiId: string | null;
  accountPlanFilter: string | null;
  aggregateApiUrl: string | null;
  protocol: string;
  clientType: string;
  authScheme: string;
  upstreamBaseUrl: string;
  staticHeadersJson: string;
  status: string;
  createdAt: number | null;
  lastUsedAt: number | null;
}

export interface ApiKeyCreateResult {
  id: string;
  key: string;
}

export interface AggregateApi {
  id: string;
  providerType: string;
  supplierName: string | null;
  sort: number;
  url: string;
  authType: string;
  authParams: Record<string, unknown> | null;
  action: string | null;
  pool: "primary" | "wool";
  woolMaxInflight: number | null;
  woolCooldownUntil: number | null;
  woolFailureCount: number;
  woolLastPreflightAt: number | null;
  fast: boolean;
  compatibilityMode: boolean;
  status: string;
  createdAt: number | null;
  updatedAt: number | null;
  lastTestAt: number | null;
  lastTestStatus: string | null;
  lastTestError: string | null;
}

export interface AggregateApiCreateResult {
  id: string;
  key: string;
}

export interface AggregateApiSecretResult {
  id: string;
  key: string;
  authType: string;
  username: string | null;
  password: string | null;
}

export interface AggregateApiTestResult {
  id: string;
  ok: boolean;
  statusCode: number | null;
  message: string | null;
  testedAt: number;
  latencyMs: number;
}

export interface AggregateApiModelUsage {
  aggregateApiUrl: string;
  model: string;
  requestCount: number;
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  reasoningOutputTokens: number;
  totalTokens: number;
  estimatedCostUsd: number;
}

export interface ApiKeyUsageStat {
  keyId: string;
  totalTokens: number;
  estimatedCostUsd: number;
}

export interface DashboardTokenUsage {
  keyId: string | null;
  keyName: string | null;
  accountId: string | null;
  accountLabel: string | null;
  aggregateApiId: string | null;
  aggregateApiSupplierName: string | null;
  aggregateApiUrl: string | null;
  model: string | null;
  requestCount: number;
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  reasoningOutputTokens: number;
  totalTokens: number;
  estimatedCostUsd: number;
  lastUsedAt: number | null;
}

export interface DashboardDailyTokenUsageBucket {
  dayStartTs: number;
  sourceKey: string;
  sourceLabel: string;
  model: string | null;
  billableInputTokens: number;
  requestCount: number;
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  reasoningOutputTokens: number;
  totalTokens: number;
  estimatedCostUsd: number;
}
