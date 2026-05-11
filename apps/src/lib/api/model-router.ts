import type {
  LatestSessionModelApplyResult,
  ModelRouteBindingListResult,
  ModelRouteBindingSavePayload,
  ModelRouteBindingSaveResult,
  ModelRouteQuickCheckResult,
  ModelRouterImportResult,
  ProbeApplySelectedPayload,
  ProbeRunAllResult,
  ProbeRunListResult,
  ProbeRunSummary,
  SessionModelListResult,
  SessionModelSummary,
  SessionModelUpdateResult,
  WorkspaceModelDefaultSummary,
} from "@/types/model-router";
import { invoke, withAddr } from "./transport";

export const modelRouterClient = {
  async listSessions(workspace?: string): Promise<SessionModelListResult> {
    return invoke<SessionModelListResult>(
      "service_model_router_session_list",
      withAddr({ workspace: workspace || null }),
    );
  },

  async updateSessionModel(params: {
    threadId: string;
    model: string;
    reasoningEffort?: string | null;
    source?: string | null;
    locked?: boolean;
  }): Promise<SessionModelUpdateResult> {
    return invoke<SessionModelUpdateResult>(
      "service_model_router_session_update_model",
      withAddr({
        threadId: params.threadId,
        model: params.model,
        reasoningEffort: params.reasoningEffort || null,
        source: params.source || "manual",
        locked: typeof params.locked === "boolean" ? params.locked : null,
      }),
    );
  },

  async applyModelToLatestWorkspaceSession(params: {
    workspace: string;
    model: string;
    reasoningEffort?: string | null;
    source?: string | null;
    locked?: boolean;
  }): Promise<LatestSessionModelApplyResult> {
    return invoke<LatestSessionModelApplyResult>(
      "service_model_router_session_apply_latest_for_workspace",
      withAddr({
        workspace: params.workspace,
        model: params.model,
        reasoningEffort: params.reasoningEffort || null,
        source: params.source || "manual",
        locked: typeof params.locked === "boolean" ? params.locked : null,
      }),
    );
  },

  async setSessionSubagentModel(params: {
    parentThreadId: string;
    model: string;
    reasoningEffort?: string | null;
    source?: string | null;
  }): Promise<SessionModelSummary> {
    return invoke<SessionModelSummary>(
      "service_model_router_session_set_subagent_model",
      withAddr({
        parentThreadId: params.parentThreadId,
        model: params.model,
        reasoningEffort: params.reasoningEffort || null,
        source: params.source || "manual",
      }),
    );
  },

  async clearSessionSubagentModel(parentThreadId: string): Promise<boolean> {
    return invoke<boolean>(
      "service_model_router_session_clear_subagent_model",
      withAddr({ parentThreadId }),
    );
  },

  async setWorkspaceDefault(params: {
    workspace: string;
    defaultModel?: string | null;
    defaultReasoningEffort?: string | null;
    inheritLastSession?: boolean;
    autoRemember?: boolean;
  }): Promise<WorkspaceModelDefaultSummary> {
    return invoke<WorkspaceModelDefaultSummary>(
      "service_model_router_workspace_default_set",
      withAddr({
        workspace: params.workspace,
        defaultModel: params.defaultModel || null,
        defaultReasoningEffort: params.defaultReasoningEffort || null,
        inheritLastSession:
          typeof params.inheritLastSession === "boolean"
            ? params.inheritLastSession
            : null,
        autoRemember:
          typeof params.autoRemember === "boolean" ? params.autoRemember : null,
      }),
    );
  },

  deleteWorkspaceDefault(workspace: string): Promise<boolean> {
    return invoke<boolean>(
      "service_model_router_workspace_default_delete",
      withAddr({ workspace }),
    );
  },

  async listBindings(model?: string): Promise<ModelRouteBindingListResult> {
    return invoke<ModelRouteBindingListResult>(
      "service_model_router_binding_list",
      withAddr({ model: model || null }),
    );
  },

  async saveBinding(
    payload: ModelRouteBindingSavePayload,
  ): Promise<ModelRouteBindingSaveResult> {
    return invoke<ModelRouteBindingSaveResult>(
      "service_model_router_binding_save",
      withAddr({
        id: payload.id || null,
        model: payload.model,
        aggregateApiId: payload.aggregateApiId,
        enabled: payload.enabled ?? null,
        priority: payload.priority ?? null,
        weight: payload.weight ?? null,
        routeStrategy: payload.routeStrategy || null,
        manualPreferred: payload.manualPreferred ?? null,
        supportsResponses: payload.supportsResponses ?? null,
        supportsChatCompletions: payload.supportsChatCompletions ?? null,
        requiresAdapter: payload.requiresAdapter ?? null,
      }),
    );
  },

  deleteBinding(id: string): Promise<unknown> {
    return invoke("service_model_router_binding_delete", withAddr({ id }));
  },

  async runProbe(
    aggregateApiId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<ProbeRunSummary> {
    return invoke<ProbeRunSummary>(
      "service_model_router_probe_run",
      withAddr({ aggregateApiId }),
      options,
    );
  },

  async runAllProbes(options: { signal?: AbortSignal } = {}): Promise<ProbeRunAllResult> {
    return invoke<ProbeRunAllResult>(
      "service_model_router_probe_run_all",
      withAddr({}),
      options,
    );
  },

  async addManualProbeModel(params: {
    aggregateApiId: string;
    model: string;
    supportsResponses?: boolean;
    supportsChatCompletions?: boolean;
    requiresAdapter?: boolean;
  }): Promise<ProbeRunSummary> {
    return invoke<ProbeRunSummary>(
      "service_model_router_probe_manual_model",
      withAddr({
        aggregateApiId: params.aggregateApiId,
        model: params.model,
        supportsResponses:
          typeof params.supportsResponses === "boolean"
            ? params.supportsResponses
            : null,
        supportsChatCompletions:
          typeof params.supportsChatCompletions === "boolean"
            ? params.supportsChatCompletions
            : null,
        requiresAdapter:
          typeof params.requiresAdapter === "boolean" ? params.requiresAdapter : null,
      }),
    );
  },

  async quickCheck(
    params: {
      aggregateApiId: string;
      model: string;
    },
    options: { signal?: AbortSignal } = {},
  ): Promise<ModelRouteQuickCheckResult> {
    return invoke<ModelRouteQuickCheckResult>(
      "service_model_router_probe_quick_call",
      withAddr({
        aggregateApiId: params.aggregateApiId,
        model: params.model,
      }),
      options,
    );
  },

  async applyProbe(probeRunId: string): Promise<ProbeRunSummary> {
    return invoke<ProbeRunSummary>(
      "service_model_router_probe_apply",
      withAddr({ probeRunId }),
    );
  },

  async applySelectedProbeCandidates(
    payload: ProbeApplySelectedPayload,
  ): Promise<ProbeRunSummary> {
    return invoke<ProbeRunSummary>(
      "service_model_router_probe_apply_selected",
      withAddr({
        probeRunId: payload.probeRunId,
        candidateIds: payload.candidateIds,
      }),
    );
  },

  async listProbeRuns(limit = 20): Promise<ProbeRunListResult> {
    return invoke<ProbeRunListResult>(
      "service_model_router_probe_list",
      withAddr({ limit }),
    );
  },

  async importCodexManager(sourcePath?: string | null): Promise<ModelRouterImportResult> {
    return invoke<ModelRouterImportResult>(
      "service_model_router_import_codexmanager",
      withAddr({ sourcePath: sourcePath || null }),
    );
  },
};
