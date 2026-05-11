"use client";

import { Fragment, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Activity,
  Bot,
  ChevronDown,
  ChevronRight,
  CheckCircle2,
  Database,
  FolderGit2,
  Gauge,
  GitBranch,
  HelpCircle,
  Info,
  Lock,
  Plus,
  RefreshCw,
  Route,
  Save,
  Search,
  Settings2,
  ShieldAlert,
  Shuffle,
  Timer,
  SlidersHorizontal,
  Unlock,
  Zap,
} from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  SearchableModelPicker,
  type SearchableModelOption,
} from "@/components/searchable-model-picker";
import { Input } from "@/components/ui/input";
import { NumberStepper } from "@/components/ui/number-stepper";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { accountClient } from "@/lib/api/account-client";
import { modelRouterClient } from "@/lib/api/model-router";
import { serviceClient } from "@/lib/api/service-client";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { useLocalDayRange } from "@/hooks/useLocalDayRange";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useAppStore } from "@/lib/store/useAppStore";
import { cn } from "@/lib/utils";
import { formatTsFromSeconds } from "@/lib/utils/usage";
import type { AggregateApi } from "@/types";
import type {
  ModelRouteBindingSummary,
  ProbeRunSummary,
  SessionModelSummary,
} from "@/types/model-router";
import type { RequestLog } from "@/types/request-log";

type RouterTab = "sessions" | "routes" | "probe" | "defaults" | "logs";

interface BindingDraft {
  id: string | null;
  model: string;
  aggregateApiId: string;
  enabled: boolean;
  priority: number;
  weight: number;
  routeStrategy: string;
  manualPreferred: boolean;
  supportsResponses: boolean;
  supportsChatCompletions: boolean;
  requiresAdapter: boolean;
}

interface DefaultDraft {
  workspace: string;
  defaultModel: string;
  defaultReasoning: string;
  inheritLastSession: boolean;
  autoRemember: boolean;
}

interface ManualModelDraft {
  aggregateApiId: string;
  model: string;
  supportsResponses: boolean;
  supportsChatCompletions: boolean;
  requiresAdapter: boolean;
}

interface SessionTreeNode {
  session: SessionModelSummary;
  children: SessionModelSummary[];
}

interface SessionTree {
  roots: SessionTreeNode[];
  orphanChildren: SessionModelSummary[];
  latestMainUpdatedAt: number;
}

interface QuickCheckViewState {
  bindingId: string;
  ok: boolean;
  protocol: string;
  latencyMs: number;
  responseAdapter: string | null;
  error: string | null;
  checkedAt: number;
}

const EMPTY_BINDING_DRAFT: BindingDraft = {
  id: null,
  model: "",
  aggregateApiId: "",
  enabled: true,
  priority: 0,
  weight: 1,
  routeStrategy: "ordered",
  manualPreferred: false,
  supportsResponses: true,
  supportsChatCompletions: true,
  requiresAdapter: false,
};

const EMPTY_MANUAL_MODEL_DRAFT: ManualModelDraft = {
  aggregateApiId: "",
  model: "",
  supportsResponses: false,
  supportsChatCompletions: true,
  requiresAdapter: true,
};

const EMPTY_DEFAULT_DRAFT: DefaultDraft = {
  workspace: "__global__",
  defaultModel: "",
  defaultReasoning: "",
  inheritLastSession: true,
  autoRemember: true,
};

const ROUTE_STRATEGY_LABELS: Record<string, string> = {
  ordered: "顺序调用",
  balanced: "均衡调用",
  manual_preferred: "手动优先",
};

const SOURCE_LABELS: Record<string, string> = {
  manual: "手动指定",
  state: "Codex 状态",
  workspace_last: "工作区上次模型",
  workspace_default: "工作区默认",
  global_default: "全局默认",
  session_override: "会话覆盖",
};

const MEMORY_STATE_LABELS: Record<string, string> = {
  memory: "已记忆",
  memory_only: "仅本地记忆",
  state: "来自状态库",
  unresolved: "未解析",
};

const REASONING_OPTIONS = [
  { value: "__none__", label: "跟随模型默认" },
  { value: "low", label: "low" },
  { value: "medium", label: "medium" },
  { value: "high", label: "high" },
  { value: "xhigh", label: "xhigh" },
];

const FALLBACK_MODEL_OPTIONS = [
  "glm-5.1",
  "gpt-5.5",
  "gpt-5.4",
  "gpt-5.4-mini",
  "kimi-k2.6",
];

function routeStrategyLabel(value: string | null | undefined): string {
  return ROUTE_STRATEGY_LABELS[String(value || "").trim()] || "顺序调用";
}

function sourceLabel(value: string | null | undefined): string {
  return SOURCE_LABELS[String(value || "").trim()] || value || "未知来源";
}

function memoryStateLabel(value: string | null | undefined): string {
  return MEMORY_STATE_LABELS[String(value || "").trim()] || value || "未知";
}

function effectiveModelLabel(session: SessionModelSummary | null | undefined): string {
  if (!session) return "未设置模型";
  return session.effectiveModelLabel || (session.hasModelOverride ? "自定义模型" : "内置模型");
}

function shortThreadId(value: string): string {
  const text = String(value || "").trim();
  if (text.length <= 14) return text;
  return `${text.slice(0, 6)}…${text.slice(-6)}`;
}

function sessionDisplayTitle(value: string | null | undefined): string {
  const text = String(value || "")
    .replace(/\s+/g, " ")
    .trim();
  if (!text) return "未命名会话";
  if (text.length <= 96) return text;
  return `${text.slice(0, 96)}…`;
}

function workspaceKey(value: string | null | undefined): string {
  return String(value || "").trim() || "__unknown_workspace__";
}

function workspaceDisplayName(value: string | null | undefined): string {
  const text = String(value || "").trim();
  if (!text) return "未识别工作区";
  const parts = text.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] || text;
}

function isApiEnabled(api: AggregateApi): boolean {
  return String(api.status || "").trim().toLowerCase() !== "disabled";
}

function apiDisplayName(api: AggregateApi | null | undefined): string {
  if (!api) return "未知上游";
  return api.supplierName || api.url || api.id;
}

function modelCategory(model: string): string {
  const value = model.trim().toLowerCase();
  if (/^(gpt-|o\d|openai)/.test(value)) return "OpenAI";
  if (value.includes("claude")) return "Claude";
  if (value.includes("gemini")) return "Gemini";
  if (
    ["glm", "qwen", "kimi", "mimo", "deepseek", "doubao", "yi", "baichuan", "hunyuan"]
      .some((prefix) => value.includes(prefix))
  ) {
    return "国产模型";
  }
  if (value.includes("custom")) return "自定义";
  return "未分类";
}

function groupedModels(models: string[]): Array<{ category: string; models: string[] }> {
  const order = ["OpenAI", "Claude", "Gemini", "国产模型", "自定义", "未分类"];
  const groups = new Map<string, string[]>();
  models.forEach((model) => {
    const category = modelCategory(model);
    groups.set(category, [...(groups.get(category) || []), model]);
  });
  return order
    .filter((category) => groups.has(category))
      .map((category) => ({
        category,
        models: Array.from(new Set(groups.get(category) || [])).sort((a, b) =>
          b.localeCompare(a),
        ),
      }));
}

function routeModelFilterValue(category: string, model?: string): string {
  return model ? `model:${model}` : `category:${category}`;
}

function parseRouteModelFilter(value: string): { type: "all" | "category" | "model"; value: string } {
  if (!value || value === "all") return { type: "all", value: "" };
  if (value.startsWith("model:")) return { type: "model", value: value.slice(6) };
  if (value.startsWith("category:")) return { type: "category", value: value.slice(9) };
  return { type: "category", value };
}

function buildThreadTree(sessions: SessionModelSummary[]): SessionTree {
  const byId = new Map(sessions.map((session) => [session.threadId, session]));
  const childrenByParent = new Map<string, SessionModelSummary[]>();
  const roots: SessionModelSummary[] = [];
  const orphanChildren: SessionModelSummary[] = [];

  sessions.forEach((session) => {
    const parentId = session.parentThreadId?.trim();
    if (session.isSubagent && parentId) {
      if (byId.has(parentId)) {
        childrenByParent.set(parentId, [
          ...(childrenByParent.get(parentId) || []),
          session,
        ]);
      } else {
        orphanChildren.push(session);
      }
      return;
    }
    roots.push(session);
  });

  const nodes = roots
    .map((session) => {
      const children = (childrenByParent.get(session.threadId) || []).sort(
        (left, right) => right.updatedAt - left.updatedAt,
      );
      return {
        session,
        children,
      };
    })
    .sort((left, right) => right.session.updatedAt - left.session.updatedAt);

  return {
    roots: nodes,
    orphanChildren: orphanChildren.sort((left, right) => right.updatedAt - left.updatedAt),
    latestMainUpdatedAt:
      roots.length > 0 ? Math.max(...roots.map((session) => session.updatedAt)) : 0,
  };
}

function statusBadge(status: string | null | undefined) {
  const normalized = String(status || "").trim().toLowerCase();
  if (normalized.includes("success")) {
    return (
      <Badge className="border-emerald-500/50 bg-emerald-500/15 text-emerald-500">
        成功
      </Badge>
    );
  }
  if (normalized.includes("fail") || normalized.includes("error")) {
    return (
      <Badge className="border-red-500/50 bg-red-500/15 text-red-500">
        失败
      </Badge>
    );
  }
  if (!normalized) return <Badge variant="secondary">未探测</Badge>;
  return <Badge variant="secondary">{status}</Badge>;
}

function routeStatusView(
  binding: ModelRouteBindingSummary,
  quickCheck: QuickCheckViewState | null | undefined,
) {
  if (quickCheck) {
    return (
      <div className="flex flex-col gap-1">
        <Badge
          className={cn(
            "w-fit gap-1 border",
            quickCheck.ok
              ? "border-emerald-500/60 bg-emerald-500/15 text-emerald-500"
              : "border-red-500/60 bg-red-500/15 text-red-500",
          )}
        >
          <Timer className="h-3 w-3" />
          {quickCheck.ok ? `${quickCheck.latencyMs} ms` : "实测失败"}
        </Badge>
        <span
          className={cn(
            "max-w-[220px] truncate text-[11px]",
            quickCheck.ok ? "text-emerald-500" : "text-red-500",
          )}
        >
          {quickCheck.ok
            ? `${quickCheck.protocol}${quickCheck.responseAdapter ? " / 内置转换" : ""}`
            : quickCheck.error || "未知错误"}
        </span>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-1">
      {binding.enabled ? (
        <Badge className="w-fit border-emerald-500/50 bg-emerald-500/15 text-emerald-500">
          已启用
        </Badge>
      ) : (
        <Badge className="w-fit border-slate-500/50 bg-slate-500/15 text-slate-400">
          已停用
        </Badge>
      )}
      {statusBadge(binding.lastProbeStatus)}
      {binding.lastSuccessAt ? (
        <span className="text-[11px] text-emerald-500">
          最近成功 {formatTsFromSeconds(binding.lastSuccessAt)}
        </span>
      ) : null}
      {binding.lastError ? (
        <span className="max-w-[220px] truncate text-[11px] text-red-500">
          {binding.lastError}
        </span>
      ) : null}
    </div>
  );
}

function capabilityBadges(binding: ModelRouteBindingSummary) {
  return (
    <div className="flex flex-wrap gap-1">
      {binding.supportsResponses ? (
        <Badge className="border-sky-500/20 bg-sky-500/10 text-sky-500">
          responses
        </Badge>
      ) : null}
      {binding.supportsChatCompletions ? (
        <Badge className="border-violet-500/20 bg-violet-500/10 text-violet-500">
          chat
        </Badge>
      ) : null}
      {binding.requiresAdapter ? (
        <Badge className="border-amber-500/20 bg-amber-500/10 text-amber-500">
          内置转换
        </Badge>
      ) : null}
      {!binding.supportsResponses && !binding.supportsChatCompletions ? (
        <Badge variant="secondary">待确认</Badge>
      ) : null}
    </div>
  );
}

function ModelRouterSkeleton() {
  return (
    <div className="space-y-5">
      <div className="grid gap-3 md:grid-cols-4">
        {Array.from({ length: 4 }).map((_, index) => (
          <Skeleton key={index} className="h-24 rounded-xl" />
        ))}
      </div>
      <Skeleton className="h-11 rounded-xl" />
      <Skeleton className="h-[460px] rounded-xl" />
    </div>
  );
}

function SessionTitle({ session }: { session: SessionModelSummary }) {
  return (
    <div className="min-w-0">
      <div className="flex min-w-0 items-center gap-2 truncate font-medium">
        {session.isSubagent ? (
          <Badge variant="secondary" className="h-5 shrink-0">子 Agent</Badge>
        ) : null}
        {session.subagentModel ? (
          <Badge className="h-5 shrink-0 border-sky-500/30 bg-sky-500/10 text-sky-600">
            子 Agent 模型
          </Badge>
        ) : null}
        <span className="truncate">
          {sessionDisplayTitle(session.title)}
        </span>
      </div>
      <div className="mt-0.5 flex flex-wrap items-center gap-2 text-[11px] text-muted-foreground">
        <span className="font-mono">{shortThreadId(session.threadId)}</span>
        {session.agentNickname ? <span>{session.agentNickname}</span> : null}
        {session.agentRole ? <span>{session.agentRole}</span> : null}
        {session.locked ? (
          <span className="inline-flex items-center gap-1 text-amber-500">
            <Lock className="h-3 w-3" /> 已锁定
          </span>
        ) : null}
      </div>
    </div>
  );
}

function SessionModelEditor({
  session,
  isPending,
  isSubagentPending,
  isClearingSubagent,
  knownModels,
  recentModels,
  onSave,
  onSaveSubagentModel,
  onClearSubagentModel,
}: {
  session: SessionModelSummary | null;
  isPending: boolean;
  isSubagentPending: boolean;
  isClearingSubagent: boolean;
  knownModels: string[];
  recentModels: string[];
  onSave: (params: {
    threadId: string;
    model: string;
    reasoningEffort?: string | null;
    locked?: boolean;
  }) => void;
  onSaveSubagentModel: (params: {
    parentThreadId: string;
    model: string;
    reasoningEffort?: string | null;
  }) => void;
  onClearSubagentModel: (parentThreadId: string) => void;
}) {
  const [model, setModel] = useState(session?.model || "");
  const [reasoningEffort, setReasoningEffort] = useState(
    session?.reasoningEffort || "",
  );
  const [subagentModel, setSubagentModel] = useState(session?.subagentModel || "");
  const [subagentReasoningEffort, setSubagentReasoningEffort] = useState(
    session?.subagentReasoningEffort || "",
  );
  const modelOptions = Array.from(
    new Set([
      ...(session?.model ? [session.model] : []),
      ...(session?.subagentModel ? [session.subagentModel] : []),
      ...recentModels,
      ...knownModels,
      ...FALLBACK_MODEL_OPTIONS,
    ]),
  ).filter(Boolean);
  const pickerOptions: SearchableModelOption[] = modelOptions.map((item) => ({
    value: item,
    label: item,
    keywords: [item],
  }));

  if (!session) {
    return (
      <div className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
        选择左侧 session 后编辑模型
      </div>
    );
  }

  return (
    <>
      <div className="rounded-lg border bg-background/40 p-3">
        <div className="truncate text-sm font-medium">
          {sessionDisplayTitle(session.title)}
        </div>
        <div className="mt-1 font-mono text-xs text-muted-foreground">
          {session.threadId}
        </div>
        <div className="mt-2 flex flex-wrap gap-2">
          <Badge
            className={
              session.hasModelOverride
                ? "border-sky-500/30 bg-sky-500/10 text-sky-600"
                : "border-border bg-background/70 text-foreground"
            }
          >
            {effectiveModelLabel(session)}
          </Badge>
          <Badge variant="secondary">
            provider: {session.modelProvider || "保持不变"}
          </Badge>
          <Badge variant="secondary">
            {sourceLabel(session.effectiveModelSource || session.source)}
          </Badge>
        </div>
      </div>
      <div className="space-y-2">
        <label className="text-xs font-medium text-muted-foreground">模型</label>
        <SearchableModelPicker
          value={model}
          onValueChange={setModel}
          options={pickerOptions}
          placeholder="选择模型"
          searchPlaceholder="搜索模型 slug"
          emptyLabel="没有匹配的模型"
          allowCustomValue
          customValuePrefix="使用输入值"
          triggerClassName="h-9 justify-between"
        />
        <div className="flex flex-wrap gap-1.5">
          {modelOptions.slice(0, 6).map((item) => (
            <Button
              key={item}
              type="button"
              variant={model === item ? "default" : "outline"}
              size="sm"
              className="h-7 px-2 font-mono text-[11px]"
              onClick={() => setModel(item)}
              >
                {item}
              </Button>
          ))}
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 px-2 text-[11px]"
            onClick={() => setModel("")}
            disabled={!model}
          >
            清除
          </Button>
        </div>
      </div>
      <div className="space-y-2">
        <label className="text-xs font-medium text-muted-foreground">
          reasoning effort
        </label>
        <Select
          value={reasoningEffort || "__none__"}
          onValueChange={(value) => {
            const next = String(value || "");
            setReasoningEffort(next === "__none__" ? "" : next);
          }}
        >
          <SelectTrigger className="w-full">
            <SelectValue placeholder="跟随模型默认" />
          </SelectTrigger>
          <SelectContent>
            {REASONING_OPTIONS.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <div className="rounded-lg border border-amber-500/50 bg-amber-500/10 px-2 py-1.5 text-[11px] text-amber-600">
          防呆：这里写入的是 Codex session 记忆。若上游不支持该 reasoning，网关会按上游实际请求失败记录到日志；下一步会把“模型能力表”接入这里做自动降级映射。
        </div>
      </div>
      <Button
        className="w-full gap-2"
        onClick={() =>
          onSave({
            threadId: session.threadId,
            model: model.trim(),
            reasoningEffort: reasoningEffort.trim() || null,
            locked: session.locked,
          })
        }
        disabled={!model.trim() || isPending}
      >
        <Save className="h-4 w-4" />
        写入该 session
      </Button>
      {!session.isSubagent ? (
        <div className="space-y-3 rounded-lg border border-sky-500/30 bg-sky-500/5 p-3">
          <div className="space-y-1">
            <div className="text-sm font-medium">新子 Agent 模型</div>
            <div className="text-[11px] text-muted-foreground">
              给这个主会话后续新发起的子 Agent 预设模型。保存后，Codex App 内通常会显示为“自定义模型”。
            </div>
          </div>
          <div className="space-y-2">
            <label className="text-xs font-medium text-muted-foreground">模型</label>
            <SearchableModelPicker
              value={subagentModel}
              onValueChange={setSubagentModel}
              options={pickerOptions}
              placeholder="选择子 Agent 模型"
              searchPlaceholder="搜索模型 slug"
              emptyLabel="没有匹配的模型"
              allowCustomValue
              customValuePrefix="使用输入值"
              triggerClassName="h-9 justify-between"
            />
            <div className="flex justify-end">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-[11px]"
                onClick={() => setSubagentModel("")}
                disabled={!subagentModel}
              >
                清除
              </Button>
            </div>
          </div>
          <div className="space-y-2">
            <label className="text-xs font-medium text-muted-foreground">
              reasoning effort
            </label>
            <Select
              value={subagentReasoningEffort || "__none__"}
              onValueChange={(value) => {
                const next = String(value || "");
                setSubagentReasoningEffort(next === "__none__" ? "" : next);
              }}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder="跟随模型默认" />
              </SelectTrigger>
              <SelectContent>
                {REASONING_OPTIONS.map((item) => (
                  <SelectItem key={item.value} value={item.value}>
                    {item.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {session.subagentModel ? (
            <div className="rounded-md border border-border/70 bg-background/70 px-2 py-1.5 text-[11px] text-muted-foreground">
              当前已设置：<span className="font-mono">{session.subagentModel}</span>
              {session.subagentReasoningEffort ? ` / ${session.subagentReasoningEffort}` : ""}
            </div>
          ) : null}
          <div className="flex gap-2">
            <Button
              className="flex-1 gap-2"
              onClick={() =>
                onSaveSubagentModel({
                  parentThreadId: session.threadId,
                  model: subagentModel.trim(),
                  reasoningEffort: subagentReasoningEffort.trim() || null,
                })
              }
              disabled={!subagentModel.trim() || isSubagentPending}
            >
              <Bot className="h-4 w-4" />
              写入新子 Agent
            </Button>
            <Button
              variant="outline"
              className="gap-2"
              onClick={() => onClearSubagentModel(session.threadId)}
              disabled={!session.subagentModel || isClearingSubagent}
            >
              清除
            </Button>
          </div>
        </div>
      ) : null}
    </>
  );
}

export default function ModelRouterPage() {
  const queryClient = useQueryClient();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const localDayRange = useLocalDayRange();
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const isPageActive = useDesktopPageActive("/model-router/");
  const isQueryEnabled = useDeferredDesktopActivation(
    isServiceReady && isPageActive,
  );

  const [activeTab, setActiveTab] = useState<RouterTab>("sessions");
  const [selectedThreadId, setSelectedThreadId] = useState<string | null>(null);
  const [bindingDraft, setBindingDraft] = useState<BindingDraft>(
    EMPTY_BINDING_DRAFT,
  );
  const [probeApiId, setProbeApiId] = useState("");
  const [defaultDraft, setDefaultDraft] = useState<DefaultDraft>(
    EMPTY_DEFAULT_DRAFT,
  );
  const [routeSearch, setRouteSearch] = useState("");
  const [collapsedWorkspaces, setCollapsedWorkspaces] = useState<string[]>([]);
  const [expandedParentThreads, setExpandedParentThreads] = useState<string[]>([]);
  const [expandedOrphanWorkspaces, setExpandedOrphanWorkspaces] = useState<string[]>([]);
  const [expandedProbeRuns, setExpandedProbeRuns] = useState<string[]>([]);
  const [selectedProbeCandidateIds, setSelectedProbeCandidateIds] = useState<
    Record<string, string[]>
  >({});
  const [routeModelFilter, setRouteModelFilter] = useState("all");
  const [quickCheckByBindingId, setQuickCheckByBindingId] = useState<
    Record<string, QuickCheckViewState>
  >({});
  const [manualModelDraft, setManualModelDraft] = useState<ManualModelDraft>(
    EMPTY_MANUAL_MODEL_DRAFT,
  );
  const prioritySaveTimers = useRef<Record<string, number>>({});
  const [importSourcePath, setImportSourcePath] = useState(
    "C:\\Users\\WIN\\AppData\\Roaming\\com.codexmanager.desktop\\codexmanager.db",
  );
  const [lastImportSummary, setLastImportSummary] = useState<string | null>(null);

  const sessionsQuery = useQuery({
    queryKey: ["model-router", "sessions"],
    queryFn: () => modelRouterClient.listSessions(),
    enabled: isQueryEnabled,
    retry: 1,
  });

  const bindingsQuery = useQuery({
    queryKey: ["model-router", "bindings"],
    queryFn: () => modelRouterClient.listBindings(),
    enabled: isQueryEnabled,
    retry: 1,
  });

  const probesQuery = useQuery({
    queryKey: ["model-router", "probes"],
    queryFn: () => modelRouterClient.listProbeRuns(20),
    enabled: isQueryEnabled,
    retry: 1,
  });

  const aggregateApisQuery = useQuery({
    queryKey: ["aggregate-apis"],
    queryFn: () => accountClient.listAggregateApis(),
    enabled: isQueryEnabled,
    retry: 1,
  });

  const requestLogsQuery = useQuery({
    queryKey: ["model-router", "request-logs"],
    queryFn: () => serviceClient.listRequestLogs({ pageSize: 50 }),
    enabled: isQueryEnabled,
    retry: 1,
  });

  const managedModelsQuery = useQuery({
    queryKey: ["managed-model-catalog", serviceStatus.addr, localDayRange.dayStartTs],
    queryFn: async () => {
      const cached = await accountClient.listManagedModels(false);
      if ((cached.items || []).length > 0) {
        return cached;
      }
      try {
        return await accountClient.listManagedModels(true);
      } catch {
        return cached;
      }
    },
    enabled: isQueryEnabled,
    retry: 1,
  });

  const sessions = useMemo(
    () => sessionsQuery.data?.items ?? [],
    [sessionsQuery.data?.items],
  );
  const bindings = useMemo(
    () => bindingsQuery.data?.items ?? [],
    [bindingsQuery.data?.items],
  );
  const probeRuns = useMemo(
    () => probesQuery.data?.items ?? [],
    [probesQuery.data?.items],
  );
  const aggregateApis = useMemo(
    () => aggregateApisQuery.data ?? [],
    [aggregateApisQuery.data],
  );
  const requestLogs = useMemo(
    () => requestLogsQuery.data?.items ?? [],
    [requestLogsQuery.data?.items],
  );
  const managedModels = useMemo(
    () => managedModelsQuery.data?.items ?? [],
    [managedModelsQuery.data?.items],
  );
  const activeApis = aggregateApis.filter(isApiEnabled);
  const firstApiId = activeApis[0]?.id ?? aggregateApis[0]?.id ?? "";
  const effectiveBindingDraft = {
    ...bindingDraft,
    aggregateApiId: bindingDraft.aggregateApiId || firstApiId,
  };
  const effectiveProbeApiId = probeApiId || firstApiId;
  const effectiveSelectedThreadId =
    selectedThreadId && sessions.some((item) => item.threadId === selectedThreadId)
      ? selectedThreadId
      : sessions.find((item) => item.locked && !item.isSubagent)?.threadId ??
        sessions.find((item) => !item.isSubagent)?.threadId ??
        sessions[0]?.threadId ??
        null;
  const selectedSession =
    sessions.find((item) => item.threadId === effectiveSelectedThreadId) ?? null;
  const apiById = useMemo(
    () => new Map(aggregateApis.map((api) => [api.id, api])),
    [aggregateApis],
  );

  const knownModels = (() => {
    const values = new Set<string>();
    managedModels.forEach((item) => {
      if (item.slug) values.add(item.slug);
    });
    FALLBACK_MODEL_OPTIONS.forEach((item) => values.add(item));
    sessions.forEach((item) => {
      if (item.model) values.add(item.model);
    });
    bindings.forEach((item) => values.add(item.model));
    probeRuns.forEach((run) => {
      run.candidates.forEach((candidate) => values.add(candidate.model));
    });
    if (sessionsQuery.data?.globalDefaultModel) {
      values.add(sessionsQuery.data.globalDefaultModel);
    }
    return Array.from(values).filter(Boolean).sort((a, b) => a.localeCompare(b));
  })();
  const recentModels = useMemo(() => {
    return Array.from(
      new Set(
        sessions
          .filter((item) => item.model)
          .sort((left, right) => right.updatedAt - left.updatedAt)
          .map((item) => item.model as string),
      ),
    );
  }, [sessions]);

  const sessionGroups = useMemo(() => {
    const groups = new Map<string, { workspace: string; sessions: SessionModelSummary[] }>();
    sessions.forEach((session) => {
      const key = workspaceKey(session.workspace);
      const existing = groups.get(key);
      if (existing) {
        existing.sessions.push(session);
      } else {
        groups.set(key, { workspace: session.workspace || "", sessions: [session] });
      }
    });
    return Array.from(groups.entries())
      .map(([key, group]) => {
        const tree = buildThreadTree(group.sessions);
        return {
          key,
          workspace: group.workspace,
          threads: tree.roots,
          orphanChildren: tree.orphanChildren,
          latestUpdatedAt:
            tree.latestMainUpdatedAt ||
            Math.max(...group.sessions.map((item) => item.updatedAt)),
          sessionCount: group.sessions.length,
          mainThreadCount: tree.roots.length,
        };
      })
      .sort((left, right) => right.latestUpdatedAt - left.latestUpdatedAt);
  }, [sessions]);

  const modelGroups = groupedModels(knownModels);
  const knownModelPickerOptions: SearchableModelOption[] = knownModels.map((item) => ({
    value: item,
    label: item,
    keywords: [item],
  }));
  const parsedRouteModelFilter = parseRouteModelFilter(routeModelFilter);

  const routeRows = (() => {
    const query = routeSearch.trim().toLowerCase();
    if (!query) return bindings;
    return bindings.filter((item) => {
      return [
        item.model,
        item.aggregateApiName,
        item.aggregateApiUrl,
        item.routeStrategy,
        item.lastError,
      ]
        .filter(Boolean)
        .some((value) => String(value).toLowerCase().includes(query));
    });
  })();
  const filteredRouteRows = routeRows
    .filter((item) => {
      if (parsedRouteModelFilter.type === "all") return true;
      if (parsedRouteModelFilter.type === "model") {
        return item.model === parsedRouteModelFilter.value;
      }
      return modelCategory(item.model) === parsedRouteModelFilter.value;
    })
    .sort((left, right) =>
      left.model.localeCompare(right.model) ||
      Number(!left.enabled) - Number(!right.enabled) ||
      left.priority - right.priority ||
      right.weight - left.weight,
    );

  const adapterBindingCount = bindings.filter((item) => item.requiresAdapter).length;
  const enabledBindingCount = bindings.filter((item) => item.enabled).length;
  const failedProbeCount = probeRuns.filter((item) => item.status === "failed").length;
  const routeRequestLogs = requestLogs
    .filter((item) => {
      const adapter = String(item.responseAdapter || "");
      return (
        item.attemptedAggregateApiIds.length > 0 ||
        adapter.includes("ResponsesFromChatCompletions")
      );
    })
    .slice(0, 8);
  const requestLogContextByConversationId = useMemo(() => {
    const map = new Map<string, { workspace: string; threadId: string }>();
    sessions.forEach((session) => {
      if (!session.threadId) return;
      map.set(session.threadId, {
        workspace: session.workspace || "-",
        threadId: session.threadId,
      });
    });
    return map;
  }, [sessions]);
  const selectedWorkspaceDefault = selectedSession
    ? sessionsQuery.data?.workspaceDefaults.find(
        (item) => item.workspace === selectedSession.workspace,
      )
    : null;
  const selectedAutoRemember = selectedWorkspaceDefault?.autoRemember ?? true;
  const latestProbeRunsByApi = useMemo(() => {
    const seen = new Set<string>();
    return probeRuns.filter((run) => {
      if (seen.has(run.aggregateApiId)) return false;
      seen.add(run.aggregateApiId);
      return true;
    });
  }, [probeRuns]);
  const isLoading =
    sessionsQuery.isLoading ||
    bindingsQuery.isLoading ||
    probesQuery.isLoading ||
    aggregateApisQuery.isLoading ||
    requestLogsQuery.isLoading;

  usePageTransitionReady("/model-router/", !isServiceReady || !isLoading);

  const invalidateModelRouter = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["model-router", "sessions"] }),
      queryClient.invalidateQueries({ queryKey: ["model-router", "bindings"] }),
      queryClient.invalidateQueries({ queryKey: ["model-router", "probes"] }),
      queryClient.invalidateQueries({ queryKey: ["model-router", "request-logs"] }),
    ]);
  };

  const updateSessionMutation = useMutation({
    mutationFn: (params: {
      threadId: string;
      model: string;
      reasoningEffort?: string | null;
      locked?: boolean;
    }) =>
      modelRouterClient.updateSessionModel({
        ...params,
        source: "manual",
      }),
    onSuccess: async (result) => {
      toast.success(result.stateUpdated ? "会话模型已写入 Codex 状态库" : "会话模型已记忆");
      setSelectedThreadId(result.item.threadId);
      await queryClient.invalidateQueries({ queryKey: ["model-router", "sessions"] });
    },
    onError: (error: unknown) => {
      toast.error(`保存会话模型失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const updateSessionSubagentMutation = useMutation({
    mutationFn: (params: {
      parentThreadId: string;
      model: string;
      reasoningEffort?: string | null;
    }) =>
      modelRouterClient.setSessionSubagentModel({
        ...params,
        source: "manual",
      }),
    onSuccess: async (result) => {
      toast.success("新子 Agent 模型已保存");
      setSelectedThreadId(result.threadId);
      await queryClient.invalidateQueries({ queryKey: ["model-router", "sessions"] });
    },
    onError: (error: unknown) => {
      toast.error(
        `保存子 Agent 模型失败: ${error instanceof Error ? error.message : String(error)}`,
      );
    },
  });

  const clearSessionSubagentMutation = useMutation({
    mutationFn: (parentThreadId: string) => modelRouterClient.clearSessionSubagentModel(parentThreadId),
    onSuccess: async () => {
      toast.success("已清除子 Agent 模型");
      await queryClient.invalidateQueries({ queryKey: ["model-router", "sessions"] });
    },
    onError: (error: unknown) => {
      toast.error(
        `清除子 Agent 模型失败: ${error instanceof Error ? error.message : String(error)}`,
      );
    },
  });

  const saveBindingMutation = useMutation({
    mutationFn: (draft: BindingDraft) =>
      modelRouterClient.saveBinding({
        id: draft.id,
        model: draft.model.trim(),
        aggregateApiId: draft.aggregateApiId,
        enabled: draft.enabled,
        priority: Number.isFinite(draft.priority) ? draft.priority : 0,
        weight: Math.max(1, Number.isFinite(draft.weight) ? draft.weight : 1),
        routeStrategy: draft.routeStrategy,
        manualPreferred: draft.manualPreferred,
        supportsResponses: draft.supportsResponses,
        supportsChatCompletions: draft.supportsChatCompletions,
        requiresAdapter: draft.requiresAdapter,
      }),
    onSuccess: async () => {
      toast.success("模型路由已保存");
      setBindingDraft((current) => ({
        ...EMPTY_BINDING_DRAFT,
        aggregateApiId: current.aggregateApiId,
      }));
      await queryClient.invalidateQueries({ queryKey: ["model-router", "bindings"] });
    },
    onError: (error: unknown) => {
      toast.error(`保存模型路由失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const deleteBindingMutation = useMutation({
    mutationFn: (id: string) => modelRouterClient.deleteBinding(id),
    onSuccess: async () => {
      toast.success("模型路由已删除");
      await queryClient.invalidateQueries({ queryKey: ["model-router", "bindings"] });
    },
    onError: (error: unknown) => {
      toast.error(`删除模型路由失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const runProbeMutation = useMutation({
    mutationFn: (aggregateApiId: string) => modelRouterClient.runProbe(aggregateApiId),
    onSuccess: async (run) => {
      toast.success(run.status === "success" ? "能力探测完成" : "能力探测完成，但存在失败项");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["model-router", "probes"] }),
        queryClient.invalidateQueries({ queryKey: ["model-router", "bindings"] }),
      ]);
    },
    onError: (error: unknown) => {
      toast.error(`能力探测失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const runAllProbesMutation = useMutation({
    mutationFn: () => modelRouterClient.runAllProbes(),
    onSuccess: async (result) => {
      toast.success(
        `已探测 ${result.attempted} 个上游：成功 ${result.succeeded}，失败 ${result.failed}`,
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["model-router", "probes"] }),
        queryClient.invalidateQueries({ queryKey: ["model-router", "bindings"] }),
      ]);
    },
    onError: (error: unknown) => {
      toast.error(`一键探测失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const manualModelMutation = useMutation({
    mutationFn: () =>
      modelRouterClient.addManualProbeModel({
        aggregateApiId: manualModelDraft.aggregateApiId || effectiveProbeApiId,
        model: manualModelDraft.model.trim(),
        supportsResponses: manualModelDraft.supportsResponses,
        supportsChatCompletions: manualModelDraft.supportsChatCompletions,
        requiresAdapter: manualModelDraft.requiresAdapter,
      }),
    onSuccess: async () => {
      toast.success("已添加手动模型候选");
      setManualModelDraft((current) => ({
        ...EMPTY_MANUAL_MODEL_DRAFT,
        aggregateApiId: current.aggregateApiId,
      }));
      await queryClient.invalidateQueries({ queryKey: ["model-router", "probes"] });
    },
    onError: (error: unknown) => {
      toast.error(`添加模型失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const quickCheckMutation = useMutation({
    mutationFn: (params: { aggregateApiId: string; model: string }) =>
      modelRouterClient.quickCheck(params),
    onSuccess: async (result) => {
      const binding = bindings.find(
        (item) =>
          item.aggregateApiId === result.aggregateApiId && item.model === result.model,
      );
      if (binding) {
        setQuickCheckByBindingId((current) => ({
          ...current,
          [binding.id]: {
            bindingId: binding.id,
            ok: result.ok,
            protocol: result.protocol,
            latencyMs: result.latencyMs,
            responseAdapter: result.responseAdapter,
            error: result.error,
            checkedAt: result.checkedAt,
          },
        }));
      }
      if (result.ok) {
        toast.success(
          `实测成功并已校正能力：${result.protocol}，${result.latencyMs}ms${
            result.responseAdapter ? "，已使用内置转换" : ""
          }`,
        );
      } else {
        toast.error(`实测失败：${result.error || result.statusCode || "未知错误"}`);
      }
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["model-router", "bindings"] }),
        queryClient.invalidateQueries({ queryKey: ["model-router", "probes"] }),
      ]);
    },
    onError: (error: unknown) => {
      toast.error(`快速实测失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const applyProbeMutation = useMutation({
    mutationFn: ({
      probeRunId,
      candidateIds,
    }: {
      probeRunId: string;
      candidateIds: string[];
    }) =>
      modelRouterClient.applySelectedProbeCandidates({
        probeRunId,
        candidateIds,
      }),
    onSuccess: async (_result, variables) => {
      setSelectedProbeCandidateIds((current) => {
        const next = { ...current };
        delete next[variables.probeRunId];
        return next;
      });
      toast.success(`已应用 ${variables.candidateIds.length} 个探测候选`);
      await invalidateModelRouter();
    },
    onError: (error: unknown) => {
      toast.error(`应用探测结果失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const importCodexManagerMutation = useMutation({
    mutationFn: () =>
      modelRouterClient.importCodexManager(importSourcePath.trim() || null),
    onSuccess: async (result) => {
      setLastImportSummary(
        `最近导入：${result.aggregateApis} 个上游、${result.aggregateApiSecrets} 个上游密钥、${result.apiKeys} 个 API key、${result.apiKeySecrets} 个 API key 密钥；备份 ${result.backupPath || "未生成"}`,
      );
      toast.success(
        `已导入 ${result.aggregateApis} 个上游、${result.apiKeys} 个 API key、${result.routeBindings} 条路由`,
      );
      await Promise.all([
        invalidateModelRouter(),
        queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] }),
      ]);
    },
    onError: (error: unknown) => {
      toast.error(`导入 CodexManager 数据失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const setDefaultMutation = useMutation({
    mutationFn: () =>
      modelRouterClient.setWorkspaceDefault({
        workspace: defaultDraft.workspace.trim() || "__global__",
        defaultModel: defaultDraft.defaultModel.trim() || null,
        defaultReasoningEffort: defaultDraft.defaultReasoning.trim() || null,
        inheritLastSession: defaultDraft.inheritLastSession,
        autoRemember: defaultDraft.autoRemember,
      }),
    onSuccess: async () => {
      toast.success("默认模型策略已保存");
      await queryClient.invalidateQueries({ queryKey: ["model-router", "sessions"] });
    },
    onError: (error: unknown) => {
      toast.error(`保存默认策略失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const deleteDefaultMutation = useMutation({
    mutationFn: (workspace: string) => modelRouterClient.deleteWorkspaceDefault(workspace),
    onSuccess: async (_, workspace) => {
      toast.success("默认模型策略已清除");
      setDefaultDraft((current) =>
        current.workspace === workspace
          ? {
              ...EMPTY_DEFAULT_DRAFT,
              workspace,
            }
          : current,
      );
      await queryClient.invalidateQueries({ queryKey: ["model-router", "sessions"] });
    },
    onError: (error: unknown) => {
      toast.error(`清除默认策略失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const applyLatestWorkspaceSessionMutation = useMutation({
    mutationFn: (params: {
      workspace: string;
      model: string;
      reasoningEffort?: string | null;
      locked?: boolean;
    }) =>
      modelRouterClient.applyModelToLatestWorkspaceSession({
        ...params,
        source: "manual",
      }),
    onSuccess: async (result) => {
      toast.success(
        result.stateUpdated
          ? `已写入最新主线程 ${shortThreadId(result.item.threadId)}`
          : `已记忆最新主线程 ${shortThreadId(result.item.threadId)}`,
      );
      setSelectedThreadId(result.item.threadId);
      await queryClient.invalidateQueries({ queryKey: ["model-router", "sessions"] });
    },
    onError: (error: unknown) => {
      toast.error(
        `应用到最新主线程失败: ${error instanceof Error ? error.message : String(error)}`,
      );
    },
  });

  const toggleSessionLock = (session: SessionModelSummary) => {
    const model = session.model || "";
    if (!model.trim()) {
      toast.error("该会话还没有可锁定的模型");
      return;
    }
    updateSessionMutation.mutate({
      threadId: session.threadId,
      model: model.trim(),
      reasoningEffort: session.reasoningEffort || null,
      locked: !session.locked,
    });
  };

  const applyDraftToLatestMainThread = () => {
    const workspace = defaultDraft.workspace.trim();
    const model = defaultDraft.defaultModel.trim();
    if (!workspace || workspace === "__global__") {
      toast.error("请先选择具体 workspace，再应用到新建 thread");
      return;
    }
    if (!model) {
      toast.error("请先选择默认模型");
      return;
    }
    applyLatestWorkspaceSessionMutation.mutate({
      workspace,
      model,
      reasoningEffort: defaultDraft.defaultReasoning.trim() || null,
      locked: false,
    });
  };

  const toggleWorkspaceCollapsed = (key: string) => {
    setCollapsedWorkspaces((current) =>
      current.includes(key)
        ? current.filter((item) => item !== key)
        : [...current, key],
    );
  };

  const toggleThreadCollapsed = (threadId: string) => {
    setExpandedParentThreads((current) =>
      current.includes(threadId)
        ? current.filter((item) => item !== threadId)
        : [...current, threadId],
    );
  };

  const toggleOrphanWorkspace = (key: string) => {
    setExpandedOrphanWorkspaces((current) =>
      current.includes(key)
        ? current.filter((item) => item !== key)
        : [...current, key],
    );
  };

  const setManualPreferredBinding = (binding: ModelRouteBindingSummary) => {
    saveBindingMutation.mutate({
      id: binding.id,
      model: binding.model,
      aggregateApiId: binding.aggregateApiId,
      enabled: binding.enabled,
      priority: binding.priority,
      weight: binding.weight,
      routeStrategy: "manual_preferred",
      manualPreferred: true,
      supportsResponses: binding.supportsResponses,
      supportsChatCompletions: binding.supportsChatCompletions,
      requiresAdapter: binding.requiresAdapter,
    });
  };

  const fillBindingDraft = (binding: ModelRouteBindingSummary) => {
    setBindingDraft({
      id: binding.id,
      model: binding.model,
      aggregateApiId: binding.aggregateApiId,
      enabled: binding.enabled,
      priority: binding.priority,
      weight: binding.weight,
      routeStrategy: binding.routeStrategy,
      manualPreferred: binding.manualPreferred,
      supportsResponses: binding.supportsResponses,
      supportsChatCompletions: binding.supportsChatCompletions,
      requiresAdapter: binding.requiresAdapter,
    });
    setActiveTab("routes");
    toast.info(`正在编辑 ${binding.model} / ${binding.aggregateApiName || binding.aggregateApiId}`);
  };

  const saveBindingPriorityLater = (binding: ModelRouteBindingSummary, nextPriority: number) => {
    window.clearTimeout(prioritySaveTimers.current[binding.id]);
    prioritySaveTimers.current[binding.id] = window.setTimeout(() => {
      saveBindingMutation.mutate({
        id: binding.id,
        model: binding.model,
        aggregateApiId: binding.aggregateApiId,
        enabled: binding.enabled,
        priority: nextPriority,
        weight: binding.weight,
        routeStrategy: binding.routeStrategy,
        manualPreferred: binding.manualPreferred,
        supportsResponses: binding.supportsResponses,
        supportsChatCompletions: binding.supportsChatCompletions,
        requiresAdapter: binding.requiresAdapter,
      });
    }, 1000);
  };

  const bumpBindingPriority = (binding: ModelRouteBindingSummary, delta: number) => {
    const nextPriority = Math.max(0, binding.priority + delta);
    setBindingDraft((current) =>
      current.id === binding.id ? { ...current, priority: nextPriority } : current,
    );
    queryClient.setQueryData(["model-router", "bindings"], (old: unknown) => {
      const oldResult = old as { items?: ModelRouteBindingSummary[] } | undefined;
      if (!oldResult?.items) return old;
      return {
        ...oldResult,
        items: oldResult.items.map((item) =>
          item.id === binding.id ? { ...item, priority: nextPriority } : item,
        ),
      };
    });
    saveBindingPriorityLater(binding, nextPriority);
  };

  const renderSessionConsoleItem = (
    session: SessionModelSummary,
    options?: { child?: boolean; childCount?: number },
  ) => {
    const selected = session.threadId === effectiveSelectedThreadId;
    const childCount = options?.childCount || 0;
    const childCollapsed = childCount > 0 && !expandedParentThreads.includes(session.threadId);
    return (
      <Fragment key={session.threadId}>
        <div
          role="button"
          tabIndex={0}
          className={cn(
            "grid w-full grid-cols-[minmax(0,1fr)_auto] gap-3 border-b-2 border-l-4 border-r-2 border-border bg-background/55 px-3 py-3 text-left transition-colors hover:bg-muted/70",
            selected &&
              "border-l-[7px] border-l-primary bg-primary/18 shadow-[inset_0_0_0_1px_rgb(var(--primary-rgb)/0.35)]",
            options?.child && "ml-7 w-[calc(100%-1.75rem)] bg-muted/45",
          )}
          onClick={() => {
            setSelectedThreadId(session.threadId);
          }}
          onKeyDown={(event) => {
            if (event.key !== "Enter" && event.key !== " ") return;
            event.preventDefault();
            setSelectedThreadId(session.threadId);
          }}
        >
          <div className="min-w-0">
            <div className="flex min-w-0 items-start gap-2">
              {childCount > 0 ? (
                <Button
                  variant="ghost"
                  size="icon-sm"
                  className="h-7 w-7 shrink-0 border border-border bg-background/80"
                  onClick={(event) => {
                    event.stopPropagation();
                    toggleThreadCollapsed(session.threadId);
                  }}
                >
                  {childCollapsed ? (
                    <ChevronRight className="h-3.5 w-3.5" />
                  ) : (
                    <ChevronDown className="h-3.5 w-3.5" />
                  )}
                </Button>
              ) : (
                <span className="h-7 w-7 shrink-0" />
              )}
              <div className="min-w-0 flex-1">
                <div className="flex min-w-0 flex-wrap items-center gap-2">
                  <SessionTitle session={session} />
                  {selected ? (
                    <Badge className="border-primary bg-primary text-primary-foreground">
                      当前选中
                    </Badge>
                  ) : null}
                </div>
                <div className="mt-2 flex flex-wrap items-center gap-2 text-[11px] text-muted-foreground">
                  <Badge variant="secondary" className="border-border bg-background/80">
                    {memoryStateLabel(session.memoryState)}
                  </Badge>
                  <Badge
                    className={
                      session.hasModelOverride
                        ? "border-sky-500/30 bg-sky-500/10 text-sky-600"
                        : "border-border bg-background/80 text-foreground"
                    }
                  >
                    {effectiveModelLabel(session)}
                  </Badge>
                  <span>{sourceLabel(session.effectiveModelSource || session.source)}</span>
                  <span>{formatTsFromSeconds(session.updatedAt)}</span>
                </div>
              </div>
            </div>
          </div>
          <div className="flex min-w-[150px] flex-col items-end gap-2">
            <div className="font-mono text-xs">{session.model || "未设置"}</div>
            {session.reasoningEffort ? (
              <div className="text-[11px] text-muted-foreground">
                reasoning: {session.reasoningEffort}
              </div>
            ) : null}
            <Button
              variant="outline"
              size="sm"
              className="h-7 gap-1 border"
              onClick={(event) => {
                event.stopPropagation();
                toggleSessionLock(session);
              }}
              disabled={updateSessionMutation.isPending}
            >
              {session.locked ? (
                <Unlock className="h-3.5 w-3.5" />
              ) : (
                <Lock className="h-3.5 w-3.5" />
              )}
              {session.locked ? "解锁" : "锁定"}
            </Button>
          </div>
        </div>
      </Fragment>
    );
  };

  return (
    <div className="animate-in space-y-3 fade-in duration-500 [&_.glass-card]:border [&_.glass-card]:border-border [&_.glass-card]:shadow-lg">
      <div className="flex flex-col gap-3 xl:flex-row xl:items-center xl:justify-between">
        <div className="flex min-w-0 items-center gap-3">
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <Route className="h-5 w-5" />
          </div>
          <div className="min-w-0">
            <h1 className="truncate text-2xl font-semibold tracking-tight">模型路由</h1>
            <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
              <Badge
                className={cn(
                  "h-6 px-2",
                  isServiceReady
                    ? "border-emerald-500/20 bg-emerald-500/10 text-emerald-500"
                    : "border-red-500/20 bg-red-500/10 text-red-500",
                )}
              >
                {isServiceReady ? "服务已连接" : "服务未连接"}
              </Badge>
              <span>{sessions.length} 个会话</span>
              <span>{enabledBindingCount}/{bindings.length} 条路由</span>
              <span>{activeApis.length}/{aggregateApis.length} 个上游启用</span>
              <span>{adapterBindingCount} 条需转换</span>
              {failedProbeCount > 0 ? <span className="text-red-500">{failedProbeCount} 个探测失败</span> : null}
            </div>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Dialog>
            <DialogTrigger render={<Button variant="outline" size="sm" className="gap-2" />}>
              <HelpCircle className="h-4 w-4" />
              使用说明
            </DialogTrigger>
            <DialogContent className="max-w-2xl">
              <DialogHeader>
                <DialogTitle>模型路由控制中心怎么用</DialogTitle>
                <DialogDescription>
                  Codex App 只保持一个 provider，当前页面按 session 写入模型、按模型绑定上游、按探测结果决定是否需要内置转换。
                </DialogDescription>
              </DialogHeader>
              <div className="grid gap-3 text-sm md:grid-cols-2">
                <div className="rounded-lg border border-border bg-background/40 p-3">
                  <div className="font-medium">会话控制</div>
                  <p className="mt-1 text-xs text-muted-foreground">“项目/工作区”就是 Codex thread 的 cwd。子 Agent 会折叠在主线程下；切模型只改 thread 的 model/reasoning，不改 provider。</p>
                </div>
                <div className="rounded-lg border border-border bg-background/40 p-3">
                  <div className="font-medium">模型路由</div>
                  <p className="mt-1 text-xs text-muted-foreground">顺序调用看优先级，均衡调用看权重，手动优先通过上游池按钮指定下一次优先 API。</p>
                </div>
                <div className="rounded-lg border border-border bg-background/40 p-3">
                  <div className="font-medium">上游探测</div>
                  <p className="mt-1 text-xs text-muted-foreground">只支持 /chat/completions 的上游需要内置转换；支持 /responses 的上游直接透传。探测会自动尝试 /models、/responses、/chat/completions；不确定时点模型上游池的“实测”校正。</p>
                </div>
                <div className="rounded-lg border border-border bg-background/40 p-3">
                  <div className="font-medium">默认设置</div>
                  <p className="mt-1 text-xs text-muted-foreground">优先级为 session 记忆、同项目上次模型、项目默认、全局默认。</p>
                </div>
              </div>
            </DialogContent>
          </Dialog>
          <Button
            variant="outline"
            size="sm"
            onClick={() => invalidateModelRouter()}
            disabled={!isServiceReady}
            className="gap-2"
          >
            <RefreshCw className="h-4 w-4" />
            刷新
          </Button>
        </div>
      </div>

      {isLoading ? (
        <ModelRouterSkeleton />
      ) : (
        <Tabs
          value={activeTab}
          onValueChange={(value) => setActiveTab(value as RouterTab)}
          className="w-full"
        >
          <TabsList className="glass-card flex h-11 w-full justify-start overflow-x-auto rounded-xl border border-border bg-background/80 p-1 no-scrollbar xl:w-fit">
            <TabsTrigger value="sessions" className="gap-2 px-4">
              <Bot className="h-4 w-4" /> 会话控制
            </TabsTrigger>
            <TabsTrigger value="routes" className="gap-2 px-4">
              <GitBranch className="h-4 w-4" /> 模型路由
            </TabsTrigger>
            <TabsTrigger value="probe" className="gap-2 px-4">
              <Search className="h-4 w-4" /> 上游探测
            </TabsTrigger>
            <TabsTrigger value="defaults" className="gap-2 px-4">
              <Settings2 className="h-4 w-4" /> 默认设置
            </TabsTrigger>
            <TabsTrigger value="logs" className="gap-2 px-4">
              <ShieldAlert className="h-4 w-4" /> 日志预览
            </TabsTrigger>
          </TabsList>

          <TabsContent value="sessions" className="space-y-4">
            <Card className="glass-card shadow-md backdrop-blur-md">
              <CardContent className="grid gap-3 text-sm md:grid-cols-4">
                <div className="min-w-0">
                  <div className="text-xs text-muted-foreground">Codex 状态库</div>
                  <div className="mt-1 flex min-w-0 items-center gap-2">
                    {sessionsQuery.data?.stateDbOk ? (
                      <Badge className="border-emerald-500/20 bg-emerald-500/10 text-emerald-500">
                        已连接
                      </Badge>
                    ) : (
                      <Badge className="border-red-500/20 bg-red-500/10 text-red-500">
                        未连接
                      </Badge>
                    )}
                    <span className="truncate text-xs text-muted-foreground">
                      {sessionsQuery.data?.stateDbPath || sessionsQuery.data?.stateDbError || "未找到 state_5.sqlite"}
                    </span>
                  </div>
                </div>
                <div className="min-w-0">
                  <div className="text-xs text-muted-foreground">全局默认</div>
                  <div className="mt-1 font-mono text-sm">
                    {sessionsQuery.data?.globalDefaultModel || "未设置"}
                  </div>
                </div>
                <div className="min-w-0 md:col-span-2">
                  <div className="text-xs text-muted-foreground">当前项目自动记忆</div>
                  <div className="mt-1 flex flex-wrap items-center gap-2 text-sm">
                    <Badge
                      className={
                        selectedAutoRemember
                          ? "border-emerald-500/20 bg-emerald-500/10 text-emerald-500"
                          : "border-amber-500/20 bg-amber-500/10 text-amber-500"
                      }
                    >
                      {selectedAutoRemember ? "已启用" : "已关闭"}
                    </Badge>
                    <span className="truncate text-xs text-muted-foreground">
                      {selectedSession?.workspace || "未选择 session"}：
                      {selectedAutoRemember
                        ? "发现 thread 时自动记忆"
                        : "只读显示，手动切换仍写入"}
                    </span>
                  </div>
                </div>
                <div className="min-w-0">
                  <div className="text-xs text-muted-foreground">当前 thread</div>
                  <div className="mt-1 truncate font-mono text-sm">
                    {selectedSession ? shortThreadId(selectedSession.threadId) : "-"}
                  </div>
                </div>
                <div className="min-w-0">
                  <div className="text-xs text-muted-foreground">更新时间</div>
                  <div className="mt-1 text-sm">
                    {selectedSession ? formatTsFromSeconds(selectedSession.updatedAt) : "-"}
                  </div>
                </div>
                <div className="min-w-0">
                  <div className="text-xs text-muted-foreground">有效模型</div>
                  <div className="mt-1 truncate text-sm">{effectiveModelLabel(selectedSession)}</div>
                </div>
                <div className="min-w-0">
                  <div className="text-xs text-muted-foreground">传输 provider</div>
                  <div className="mt-1 truncate text-sm">
                    {selectedSession?.modelProvider || "保持不变"}
                  </div>
                </div>
                <div className="min-w-0">
                  <div className="text-xs text-muted-foreground">父线程</div>
                  <div className="mt-1 truncate font-mono text-sm">
                    {selectedSession?.parentThreadId
                      ? shortThreadId(selectedSession.parentThreadId)
                      : "-"}
                  </div>
                </div>
              </CardContent>
            </Card>

            <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
              <Card className="glass-card shadow-md backdrop-blur-md">
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <Database className="h-4 w-4" /> 当前 session / thread
                  </CardTitle>
                </CardHeader>
                <CardContent>
                  {sessions.length === 0 ? (
                    <div className="rounded-lg border border-dashed border-border p-8 text-center text-sm text-muted-foreground">
                      尚未读取到 Codex session
                    </div>
                  ) : (
                    <div className="overflow-hidden rounded-lg border border-border bg-background/40">
                      {sessionGroups.map((group) => {
                        const collapsed = collapsedWorkspaces.includes(group.key);
                        const orphanExpanded = expandedOrphanWorkspaces.includes(group.key);
                        return (
                          <section key={group.key} className="border-b-4 border-border last:border-b-0">
                            <button
                              type="button"
                              className="flex w-full items-center justify-between gap-3 border-b-2 border-border bg-muted/70 px-3 py-3 text-left transition-colors hover:bg-muted"
                              onClick={() => toggleWorkspaceCollapsed(group.key)}
                            >
                              <div className="flex min-w-0 items-center gap-2">
                                {collapsed ? (
                                  <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />
                                ) : (
                                  <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground" />
                                )}
                                <FolderGit2 className="h-4 w-4 shrink-0 text-primary" />
                                <div className="min-w-0">
                                  <div className="truncate font-medium">
                                    {workspaceDisplayName(group.workspace)}
                                  </div>
                                  <div className="truncate font-mono text-[11px] text-muted-foreground">
                                    {group.workspace || "unknown"}
                                  </div>
                                </div>
                              </div>
                              <div className="flex shrink-0 items-center gap-2">
                                <Badge variant="secondary">{group.mainThreadCount} 个主线程</Badge>
                                <Badge variant="secondary">{group.sessionCount} 个 session</Badge>
                                <span className="text-[11px] text-muted-foreground">
                                  {formatTsFromSeconds(group.latestUpdatedAt)}
                                </span>
                              </div>
                            </button>
                            {collapsed ? null : (
                              <div className="divide-y-2 divide-border">
                                {group.threads.length === 0 ? (
                                  <div className="px-3 py-4 text-sm text-muted-foreground">
                                    未读取到主线程
                                  </div>
                                ) : (
                                  group.threads.flatMap((node) => {
                                    const rows = [
                                      renderSessionConsoleItem(node.session, {
                                        childCount: node.children.length,
                                      }),
                                    ];
                                    const childrenCollapsed = !expandedParentThreads.includes(
                                      node.session.threadId,
                                    );
                                    if (!childrenCollapsed) {
                                      rows.push(
                                        ...node.children.map((child) =>
                                          renderSessionConsoleItem(child, { child: true }),
                                        ),
                                      );
                                    }
                                    return rows;
                                  })
                                )}
                                {group.orphanChildren.length > 0 ? (
                                  <div className="border-t-4 border-border">
                                    <button
                                      type="button"
                                      className="flex w-full items-center justify-between gap-3 border-b-2 border-border bg-amber-500/10 px-3 py-2 text-left hover:bg-amber-500/15"
                                      onClick={() => toggleOrphanWorkspace(group.key)}
                                    >
                                      <div className="flex min-w-0 items-center gap-2">
                                        {orphanExpanded ? (
                                          <ChevronDown className="h-4 w-4 shrink-0" />
                                        ) : (
                                          <ChevronRight className="h-4 w-4 shrink-0" />
                                        )}
                                        <Bot className="h-4 w-4 shrink-0 text-amber-600" />
                                        <span className="truncate text-sm font-medium">
                                          未归属子 Agent
                                        </span>
                                      </div>
                                      <Badge className="border-amber-500/50 bg-amber-500/15 text-amber-700">
                                        {group.orphanChildren.length} 个
                                      </Badge>
                                    </button>
                                    {orphanExpanded
                                      ? group.orphanChildren.map((child) =>
                                          renderSessionConsoleItem(child, { child: true }),
                                        )
                                      : null}
                                  </div>
                                ) : null}
                              </div>
                            )}
                          </section>
                        );
                      })}
                    </div>
                  )}
                </CardContent>
              </Card>

              <Card className="glass-card shadow-md backdrop-blur-md xl:sticky xl:top-4">
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <SlidersHorizontal className="h-4 w-4" /> 当前会话模型
                  </CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                  <SessionModelEditor
                    key={`${selectedSession?.threadId || "empty-session"}:${selectedSession?.updatedAt || 0}`}
                    session={selectedSession}
                    isPending={updateSessionMutation.isPending}
                    isSubagentPending={updateSessionSubagentMutation.isPending}
                    isClearingSubagent={clearSessionSubagentMutation.isPending}
                    knownModels={knownModels}
                    recentModels={recentModels}
                    onSave={(params) => updateSessionMutation.mutate(params)}
                    onSaveSubagentModel={(params) =>
                      updateSessionSubagentMutation.mutate(params)
                    }
                    onClearSubagentModel={(parentThreadId) =>
                      clearSessionSubagentMutation.mutate(parentThreadId)
                    }
                  />
                </CardContent>
              </Card>
            </div>
          </TabsContent>

          <TabsContent value="routes" className="space-y-4">
            <div className="grid gap-4 xl:grid-cols-[360px_minmax(0,1fr)]">
              <Card className="glass-card shadow-md backdrop-blur-md">
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <GitBranch className="h-4 w-4" />
                    {effectiveBindingDraft.id ? "编辑路由绑定" : "新增路由绑定"}
                  </CardTitle>
                </CardHeader>
                <CardContent className="space-y-3">
                  <div className="space-y-1.5">
                    <label className="flex items-center gap-1 text-xs text-muted-foreground">
                      模型
                      <Info className="h-3.5 w-3.5" />
                    </label>
                    <Select
                      value={effectiveBindingDraft.model || "__none__"}
                      onValueChange={(value) => {
                        const next = String(value || "");
                        setBindingDraft((current) => ({
                          ...current,
                          model: next === "__none__" ? "" : next,
                        }));
                      }}
                    >
                      <SelectTrigger>
                        <SelectValue placeholder="选择模型" />
                      </SelectTrigger>
                      <SelectContent className="min-w-72">
                        <SelectItem value="__none__">选择模型</SelectItem>
                        {modelGroups.map((group) => (
                          <SelectGroup key={group.category}>
                            <SelectLabel>{group.category}</SelectLabel>
                            {group.models.map((model) => (
                              <SelectItem key={model} value={model}>
                                {model}
                              </SelectItem>
                            ))}
                          </SelectGroup>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <Select
                    value={effectiveBindingDraft.aggregateApiId || "__none__"}
                    onValueChange={(value) =>
                      setBindingDraft((current) => ({
                        ...current,
                        aggregateApiId: String(value || "") === "__none__" ? "" : String(value || ""),
                      }))
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue>
                        {(value) => {
                          const id = String(value || "");
                          const api = apiById.get(id);
                          return api ? apiDisplayName(api) : "选择上游 API";
                        }}
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="__none__">选择上游 API</SelectItem>
                      {aggregateApis.map((api) => (
                        <SelectItem key={api.id} value={api.id}>
                          {apiDisplayName(api)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Select
                    value={effectiveBindingDraft.routeStrategy}
                    onValueChange={(value) =>
                      setBindingDraft((current) => ({
                        ...current,
                        routeStrategy: String(value || "ordered"),
                      }))
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue>
                        {(value) => routeStrategyLabel(String(value || ""))}
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="ordered">顺序调用</SelectItem>
                      <SelectItem value="balanced">均衡调用</SelectItem>
                      <SelectItem value="manual_preferred">手动优先</SelectItem>
                    </SelectContent>
                  </Select>
                  {effectiveBindingDraft.routeStrategy === "balanced" ? (
                    <div className="space-y-1">
                      <label className="text-xs text-muted-foreground">均衡权重</label>
                      <NumberStepper
                        value={effectiveBindingDraft.weight}
                        min={1}
                        ariaLabel="均衡权重"
                        disabled={saveBindingMutation.isPending}
                        onCommit={(value) =>
                          setBindingDraft((current) => ({
                            ...current,
                            weight: value,
                          }))
                        }
                      />
                      <p className="text-[11px] text-muted-foreground">
                        权重越高，被均衡策略选中的概率越高。
                      </p>
                    </div>
                  ) : (
                    <div className="space-y-1">
                      <label className="text-xs text-muted-foreground">顺序优先级</label>
                      <NumberStepper
                        value={effectiveBindingDraft.priority}
                        ariaLabel="顺序优先级"
                        disabled={saveBindingMutation.isPending}
                        onCommit={(value) =>
                          setBindingDraft((current) => ({
                            ...current,
                            priority: value,
                          }))
                        }
                      />
                      <p className="text-[11px] text-muted-foreground">
                        数字越小越先尝试；失败后只在该模型上游池内回退。
                      </p>
                    </div>
                  )}
                  <div className="grid gap-2 rounded-lg border border-border bg-background/40 p-3">
                    <div className="flex items-center justify-between gap-3">
                      <div className="min-w-0">
                        <span className="text-xs text-muted-foreground">启用绑定</span>
                        <p className="text-[11px] text-muted-foreground">
                          关闭后该模型不会路由到这个上游，fallback 也不会使用它。
                        </p>
                      </div>
                      <Switch
                        checked={effectiveBindingDraft.enabled}
                        onCheckedChange={(checked) =>
                          setBindingDraft((current) => ({ ...current, enabled: checked }))
                        }
                      />
                    </div>
                    <div className="rounded-md bg-amber-500/10 px-2 py-1.5 text-[11px] text-amber-600">
                      能力字段优先由探测和快速实测更新。只有确认上游行为时才手动覆盖。
                    </div>
                    {[
                      ["supportsResponses", "支持 /responses"],
                      ["supportsChatCompletions", "支持 /chat/completions"],
                      ["requiresAdapter", "需要内置转换"],
                    ].map(([key, label]) => (
                      <div key={key} className="flex items-center justify-between gap-3">
                        <span className="text-xs text-muted-foreground">{label}</span>
                        <Switch
                          checked={Boolean(effectiveBindingDraft[key as keyof BindingDraft])}
                          onCheckedChange={(checked) =>
                            setBindingDraft((current) => ({
                              ...current,
                              [key]: checked,
                            }))
                          }
                        />
                      </div>
                    ))}
                  </div>
                  <div className="flex gap-2">
                    <Button
                      className="flex-1 gap-2"
                      onClick={() => saveBindingMutation.mutate(effectiveBindingDraft)}
                      disabled={
                        !effectiveBindingDraft.model.trim() ||
                        !effectiveBindingDraft.aggregateApiId ||
                        saveBindingMutation.isPending
                      }
                    >
                      <Save className="h-4 w-4" />
                      保存
                    </Button>
                    <Button
                      variant="outline"
                      onClick={() =>
                        setBindingDraft({
                          ...EMPTY_BINDING_DRAFT,
                          aggregateApiId: effectiveBindingDraft.aggregateApiId,
                        })
                      }
                    >
                      清空
                    </Button>
                  </div>
                </CardContent>
              </Card>

              <Card className="glass-card shadow-md backdrop-blur-md">
                <CardHeader className="gap-3 md:flex-row md:items-center md:justify-between">
                  <CardTitle className="flex items-center gap-2">
                    <Shuffle className="h-4 w-4" /> 模型上游池
                  </CardTitle>
                  <div className="grid w-full gap-2 md:w-[560px] md:grid-cols-[220px_minmax(0,1fr)]">
                    <Select
                      value={routeModelFilter}
                      onValueChange={(value) => setRouteModelFilter(String(value || "all"))}
                    >
                      <SelectTrigger>
                        <SelectValue placeholder="模型分类/具体模型" />
                      </SelectTrigger>
                      <SelectContent className="min-w-72">
                        <SelectItem value="all">全部模型</SelectItem>
                        {modelGroups.map((group) => (
                          <SelectGroup key={group.category}>
                            <SelectLabel>{group.category}</SelectLabel>
                            <SelectItem value={routeModelFilterValue(group.category)}>
                              全部 {group.category}
                            </SelectItem>
                            {group.models.map((model) => (
                              <SelectItem
                                key={model}
                                value={routeModelFilterValue(group.category, model)}
                              >
                                {model}
                              </SelectItem>
                            ))}
                          </SelectGroup>
                        ))}
                      </SelectContent>
                    </Select>
                    <div className="relative">
                      <Search className="pointer-events-none absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
                      <Input
                        value={routeSearch}
                        onChange={(event) => setRouteSearch(event.target.value)}
                        className="pl-8"
                        placeholder="搜索模型、上游、错误"
                      />
                    </div>
                  </div>
                </CardHeader>
                <CardContent className="overflow-x-auto">
                  <Table className="[&_td]:border-b-2 [&_td]:border-border [&_th]:border-b-2 [&_th]:border-border">
                    <TableHeader>
                    <TableRow>
                        <TableHead>模型</TableHead>
                        <TableHead>上游</TableHead>
                        <TableHead>策略</TableHead>
                        <TableHead>顺序/权重</TableHead>
                        <TableHead>能力</TableHead>
                        <TableHead>状态</TableHead>
                        <TableHead className="text-right">操作</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {filteredRouteRows.length === 0 ? (
                      <TableRow>
                          <TableCell colSpan={7} className="h-24 text-center text-muted-foreground">
                            尚未配置模型路由
                          </TableCell>
                        </TableRow>
                      ) : (
                        filteredRouteRows.map((binding) => (
                        <TableRow
                            key={binding.id}
                            className={cn(
                              "border-b-2 border-border hover:bg-muted/55",
                              effectiveBindingDraft.id === binding.id && "bg-primary/8 ring-1 ring-primary/25",
                              !binding.enabled && "opacity-70",
                            )}
                          >
                            <TableCell className="font-mono text-xs">
                              {binding.model}
                            </TableCell>
                            <TableCell className="max-w-[260px]">
                              <div className="truncate font-medium">
                                {binding.aggregateApiName || "未命名上游"}
                              </div>
                              <div className="truncate text-[11px] text-muted-foreground">
                                {binding.aggregateApiUrl || binding.aggregateApiId}
                              </div>
                            </TableCell>
                            <TableCell>
                              <div className="flex flex-wrap gap-1">
                                <Badge variant="secondary">
                                  {routeStrategyLabel(binding.routeStrategy)}
                                </Badge>
                                {binding.manualPreferred ? (
                                  <Badge className="border-amber-500/20 bg-amber-500/10 text-amber-500">
                                    手动优先
                                  </Badge>
                                ) : null}
                              </div>
                            </TableCell>
                            <TableCell className="text-xs">
                              {binding.routeStrategy === "balanced" ? (
                                <span>weight {binding.weight}</span>
                              ) : (
                                <div className="inline-flex items-center rounded-md border border-border bg-background/50">
                                  <Button
                                    variant="ghost"
                                    size="icon-sm"
                                    className="h-7 w-7 rounded-r-none"
                                    onClick={() => bumpBindingPriority(binding, -1)}
                                    disabled={saveBindingMutation.isPending}
                                  >
                                    -
                                  </Button>
                                  <span className="min-w-10 border-x-2 border-border px-2 text-center font-mono">
                                    {binding.priority}
                                  </span>
                                  <Button
                                    variant="ghost"
                                    size="icon-sm"
                                    className="h-7 w-7 rounded-l-none"
                                    onClick={() => bumpBindingPriority(binding, 1)}
                                    disabled={saveBindingMutation.isPending}
                                  >
                                    +
                                  </Button>
                                </div>
                              )}
                            </TableCell>
                            <TableCell>{capabilityBadges(binding)}</TableCell>
                            <TableCell>{routeStatusView(binding, quickCheckByBindingId[binding.id])}</TableCell>
                            <TableCell className="text-right">
                              <div className="flex justify-end gap-1">
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  className="gap-1"
                                  onClick={() =>
                                    quickCheckMutation.mutate({
                                      aggregateApiId: binding.aggregateApiId,
                                      model: binding.model,
                                    })
                                  }
                                  disabled={quickCheckMutation.isPending}
                                >
                                  <Gauge className="h-3.5 w-3.5" />
                                  实测
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => setManualPreferredBinding(binding)}
                                  disabled={saveBindingMutation.isPending}
                                >
                                  下次优先
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => fillBindingDraft(binding)}
                                >
                                  编辑
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  className="text-red-500 hover:text-red-500"
                                  onClick={() => deleteBindingMutation.mutate(binding.id)}
                                  disabled={deleteBindingMutation.isPending}
                                >
                                  删除
                                </Button>
                              </div>
                            </TableCell>
                          </TableRow>
                        ))
                      )}
                    </TableBody>
                  </Table>
                </CardContent>
              </Card>
            </div>
          </TabsContent>

          <TabsContent value="probe" className="space-y-4">
            <Card className="glass-card shadow-md backdrop-blur-md">
              <CardHeader>
                <CardTitle className="flex items-center gap-2">
                  <Search className="h-4 w-4" /> 聚合 API 能力探测
                </CardTitle>
              </CardHeader>
              <CardContent className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto_auto] lg:items-end">
                <div className="space-y-2">
                  <label className="text-xs font-medium text-muted-foreground">
                    选择要探测的上游
                  </label>
                  <Select
                    value={effectiveProbeApiId || "__none__"}
                    onValueChange={(value) =>
                      setProbeApiId(
                        String(value || "") === "__none__" ? "" : String(value || ""),
                      )
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue>
                        {(value) => {
                          const api = apiById.get(String(value || ""));
                          return api ? apiDisplayName(api) : "选择上游 API";
                        }}
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="__none__">选择上游 API</SelectItem>
                      {aggregateApis.map((api) => (
                        <SelectItem key={api.id} value={api.id}>
                          {apiDisplayName(api)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <Button
                  className="gap-2"
                  onClick={() => runProbeMutation.mutate(effectiveProbeApiId)}
                  disabled={!effectiveProbeApiId || runProbeMutation.isPending}
                >
                  <Zap className="h-4 w-4" />
                  探测模型能力
                </Button>
                <Button
                  variant="outline"
                  className="gap-2"
                  onClick={() => runAllProbesMutation.mutate()}
                  disabled={runAllProbesMutation.isPending || aggregateApis.length === 0}
                >
                  <RefreshCw className={cn("h-4 w-4", runAllProbesMutation.isPending && "animate-spin")} />
                  一键探测全部
                </Button>
              </CardContent>
            </Card>

            <Card className="glass-card shadow-md backdrop-blur-md">
              <CardHeader>
                <CardTitle className="flex items-center gap-2">
                  <Plus className="h-4 w-4" /> 手动添加模型到上游
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <div className="rounded-lg border border-border bg-background/40 px-3 py-2 text-xs text-muted-foreground">
                  自动探测会尝试 /responses 和 /chat/completions。若 /responses 不通但 /chat/completions 可用，该模型会标记为“需要转换”，网关内部自动把 Codex 的 /responses 请求转换成 chat completions。
                </div>
                <div className="grid gap-3 lg:grid-cols-[minmax(0,320px)_minmax(0,1fr)_auto] lg:items-start">
                <div className="space-y-2">
                  <label className="text-xs font-medium text-muted-foreground">上游 API</label>
                  <Select
                    value={manualModelDraft.aggregateApiId || effectiveProbeApiId || "__none__"}
                    onValueChange={(value) =>
                      setManualModelDraft((current) => ({
                        ...current,
                        aggregateApiId: String(value || "") === "__none__" ? "" : String(value || ""),
                      }))
                    }
                  >
                    <SelectTrigger>
                      <SelectValue>
                        {(value) => {
                          const api = apiById.get(String(value || ""));
                          return api ? apiDisplayName(api) : "选择上游 API";
                        }}
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="__none__">选择上游 API</SelectItem>
                      {aggregateApis.map((api) => (
                        <SelectItem key={api.id} value={api.id}>
                          {apiDisplayName(api)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="space-y-2">
                  <label className="text-xs font-medium text-muted-foreground">模型</label>
                  <Input
                    value={manualModelDraft.model}
                    onChange={(event) =>
                      setManualModelDraft((current) => ({
                        ...current,
                        model: event.target.value,
                      }))
                    }
                    placeholder="例如 glm-5.1"
                    list="model-router-known-models"
                  />
                  <div className="flex flex-wrap gap-2">
                    <Badge variant={manualModelDraft.supportsResponses ? "default" : "secondary"} className="cursor-pointer" onClick={() => setManualModelDraft((current) => ({ ...current, supportsResponses: !current.supportsResponses }))}>responses</Badge>
                    <Badge variant={manualModelDraft.supportsChatCompletions ? "default" : "secondary"} className="cursor-pointer" onClick={() => setManualModelDraft((current) => ({ ...current, supportsChatCompletions: !current.supportsChatCompletions }))}>chat</Badge>
                    <Badge variant={manualModelDraft.requiresAdapter ? "default" : "secondary"} className="cursor-pointer" onClick={() => setManualModelDraft((current) => ({ ...current, requiresAdapter: !current.requiresAdapter }))}>需要转换</Badge>
                  </div>
                </div>
                <Button
                  className="gap-2"
                  onClick={() => manualModelMutation.mutate()}
                  disabled={
                    manualModelMutation.isPending ||
                    !manualModelDraft.model.trim() ||
                    !(manualModelDraft.aggregateApiId || effectiveProbeApiId)
                  }
                >
                  <Save className="h-4 w-4" />
                  添加候选
                </Button>
                </div>
              </CardContent>
            </Card>

            <Card className="glass-card shadow-md backdrop-blur-md">
              <CardHeader>
                <CardTitle className="flex items-center gap-2">
                  <CheckCircle2 className="h-4 w-4" /> 探测结果预览
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                {latestProbeRunsByApi.length === 0 ? (
                  <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
                    还没有探测记录。探测后会先生成候选路由，确认应用后才写入路由表。
                  </div>
                ) : (
                  latestProbeRunsByApi.map((run) => {
                    const validCandidateIds = run.candidates
                      .filter(
                        (candidate) =>
                          !candidate.applied &&
                          !candidate.error &&
                          (candidate.supportsResponses || candidate.supportsChatCompletions),
                      )
                      .map((candidate) => candidate.id);
                    const selectedCandidateIds =
                      selectedProbeCandidateIds[run.id] ?? validCandidateIds;
                    return (
                    <div key={run.id} className="rounded-lg border border-border bg-background/45 p-3">
                      <div className="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="font-medium">
                              {run.aggregateApiName || run.aggregateApiId}
                            </span>
                            {statusBadge(run.status)}
                          </div>
                          <div className="mt-1 flex flex-wrap gap-2 text-[11px] text-muted-foreground">
                            <span>/models: {run.modelsStatus || "未执行"}</span>
                            <span>/responses: {run.responsesStatus || "未执行"}</span>
                            <span>/chat/completions: {run.chatCompletionsStatus || "未执行"}</span>
                          </div>
                        </div>
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() => {
                            if (selectedCandidateIds.length === 0) {
                              toast.info("请先选择要应用的候选模型");
                              return;
                            }
                            applyProbeMutation.mutate({
                              probeRunId: run.id,
                              candidateIds: selectedCandidateIds,
                            });
                          }}
                          disabled={
                            selectedCandidateIds.length === 0 ||
                            applyProbeMutation.isPending
                          }
                        >
                          应用所选 {selectedCandidateIds.length}
                        </Button>
                      </div>
                      {run.error ? (
                        <div className="mt-2 rounded-md bg-red-500/10 px-2 py-1 text-xs text-red-500">
                          {run.error}
                        </div>
                      ) : null}
                      <div className="mt-3 grid gap-2 md:grid-cols-2 xl:grid-cols-3">
                        {(expandedProbeRuns.includes(run.id) ? run.candidates : run.candidates.slice(0, 3)).map((candidate) => (
                          <div
                            key={candidate.id}
                            className="rounded-lg border bg-card/60 p-2"
                          >
                            <div className="flex items-center justify-between gap-2">
                              <label className="flex min-w-0 cursor-pointer items-center gap-2">
                                <Checkbox
                                  checked={selectedCandidateIds.includes(candidate.id)}
                                  disabled={
                                    candidate.applied ||
                                    Boolean(candidate.error) ||
                                    (!candidate.supportsResponses &&
                                      !candidate.supportsChatCompletions)
                                  }
                                  onCheckedChange={(checked) =>
                                    setSelectedProbeCandidateIds((current) => {
                                      const next = new Set(selectedCandidateIds);
                                      if (checked) next.add(candidate.id);
                                      else next.delete(candidate.id);
                                      return {
                                        ...current,
                                        [run.id]: Array.from(next),
                                      };
                                    })
                                  }
                                />
                                <span className="truncate font-mono text-xs">
                                  {candidate.model}
                                </span>
                              </label>
                              {candidate.applied ? (
                                <Badge className="border-emerald-500/20 bg-emerald-500/10 text-emerald-500">
                                  已应用
                                </Badge>
                              ) : (
                                <Badge variant="secondary">待确认</Badge>
                              )}
                            </div>
                            <div className="mt-2 flex flex-wrap gap-1">
                              {candidate.supportsResponses ? <Badge variant="secondary">responses</Badge> : null}
                              {candidate.supportsChatCompletions ? <Badge variant="secondary">chat</Badge> : null}
                              {candidate.requiresAdapter ? (
                                <Badge className="border-amber-500/20 bg-amber-500/10 text-amber-500">
                                  转换
                                </Badge>
                              ) : null}
                            </div>
                            {candidate.error ? (
                              <p className="mt-2 text-[11px] text-red-500">
                                {candidate.error}
                              </p>
                            ) : null}
                          </div>
                        ))}
                      </div>
                      {run.candidates.length > 3 ? (
                        <Button
                          variant="outline"
                          size="sm"
                          className="mt-3"
                          onClick={() =>
                            setExpandedProbeRuns((current) =>
                              current.includes(run.id)
                                ? current.filter((item) => item !== run.id)
                                : [...current, run.id],
                            )
                          }
                        >
                          {expandedProbeRuns.includes(run.id)
                            ? "收起模型"
                            : `展开其余 ${run.candidates.length - 3} 个模型`}
                        </Button>
                      ) : null}
                    </div>
                    );
                  })
                )}
              </CardContent>
            </Card>
          </TabsContent>

          <TabsContent value="defaults" className="space-y-4">
            <Card className="glass-card border border-border shadow-md backdrop-blur-md">
              <CardContent className="grid gap-3 text-sm md:grid-cols-3">
                <div className="rounded-lg border bg-background/30 p-3">
                  <div className="font-medium">默认优先级</div>
                  <p className="mt-1 text-xs text-muted-foreground">
                    session 记忆优先，其次同项目/工作区上一次模型，再到项目默认，最后全局默认。
                  </p>
                </div>
                <div className="rounded-lg border bg-background/30 p-3">
                  <div className="font-medium">自动记忆开关</div>
                  <p className="mt-1 text-xs text-muted-foreground">
                    关闭后刷新列表只读 Codex state，不自动覆盖 session memory；手动切换仍会写入。
                  </p>
                </div>
                <div className="rounded-lg border bg-background/30 p-3">
                  <div className="font-medium">导入用于验收</div>
                  <p className="mt-1 text-xs text-muted-foreground">
                    从现有 CodexManager DB 复制上游、密钥和 API key 到当前验收库，源库只读。
                  </p>
                </div>
              </CardContent>
            </Card>

            <Card className="glass-card border border-border shadow-md backdrop-blur-md">
              <CardHeader>
                <CardTitle className="flex items-center gap-2">
                  <Database className="h-4 w-4" /> 从现有 CodexManager 导入
                </CardTitle>
              </CardHeader>
              <CardContent className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto]">
                <div className="space-y-1">
                  <Input
                    value={importSourcePath}
                    onChange={(event) => setImportSourcePath(event.target.value)}
                    placeholder="C:\Users\WIN\AppData\Roaming\com.codexmanager.desktop\codexmanager.db"
                  />
                  <p className="text-xs text-muted-foreground">
                    会复制聚合 API、密钥、API key、已有模型路由和默认策略；导入前自动备份当前验收数据库，不写入源数据库。
                  </p>
                  {lastImportSummary ? (
                    <p className="rounded-md border border-emerald-500/20 bg-emerald-500/10 px-2 py-1 text-xs text-emerald-500">
                      {lastImportSummary}
                    </p>
                  ) : null}
                </div>
                <Button
                  className="gap-2"
                  onClick={() => importCodexManagerMutation.mutate()}
                  disabled={importCodexManagerMutation.isPending}
                >
                  <Database className="h-4 w-4" />
                  导入配置/API
                </Button>
              </CardContent>
            </Card>

            <div className="grid gap-4 xl:grid-cols-[380px_minmax(0,1fr)]">
              <Card className="glass-card border border-border shadow-md backdrop-blur-md">
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <Settings2 className="h-4 w-4" /> 默认模型策略
                  </CardTitle>
                </CardHeader>
                <CardContent className="space-y-3">
                  <div className="space-y-2">
                    <label className="text-xs font-medium text-muted-foreground">
                      项目/工作区（cwd）
                    </label>
                    <Select
                      value={defaultDraft.workspace || "__global__"}
                      onValueChange={(value) => {
                        setDefaultDraft((current) => ({
                          ...current,
                          workspace: String(value || "__global__"),
                        }));
                      }}
                    >
                      <SelectTrigger className="w-full">
                        <SelectValue placeholder="从当前 session 项目选择" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="__global__">全局默认（所有项目兜底）</SelectItem>
                        {sessionGroups.map((group) => (
                          <SelectItem key={group.key} value={group.workspace || group.key}>
                            {workspaceDisplayName(group.workspace)} · {group.sessionCount} 个 session
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <Input
                      value={defaultDraft.workspace}
                      onChange={(event) =>
                        setDefaultDraft((current) => ({
                          ...current,
                          workspace: event.target.value,
                        }))
                      }
                      placeholder="__global__ 或 workspace 路径"
                    />
                    <p className="text-[11px] text-muted-foreground">
                      这里的项目/工作区就是 Codex thread 记录的 cwd；不知道填什么时，直接从上面的当前 session 项目列表选择。
                    </p>
                  </div>
                  <div className="space-y-2">
                    <label className="text-xs font-medium text-muted-foreground">
                      默认模型
                    </label>
                    <SearchableModelPicker
                      value={defaultDraft.defaultModel}
                      onValueChange={(next) =>
                        setDefaultDraft((current) => ({
                          ...current,
                          defaultModel: next,
                        }))
                      }
                      options={knownModelPickerOptions}
                      placeholder="不设置"
                      searchPlaceholder="搜索默认模型"
                      emptyLabel="没有匹配的模型"
                      allowCustomValue
                      customValuePrefix="使用输入值"
                      triggerClassName="h-9 justify-between"
                    />
                    <div className="flex items-center justify-end gap-2">
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-7 px-2 text-[11px]"
                        onClick={() =>
                          setDefaultDraft((current) => ({
                            ...current,
                            defaultModel: "",
                          }))
                        }
                        disabled={!defaultDraft.defaultModel.trim()}
                      >
                        清空模型
                      </Button>
                    </div>
                  </div>
                  <div className="space-y-2">
                    <label className="text-xs font-medium text-muted-foreground">
                      默认 reasoning effort
                    </label>
                    <Select
                      value={defaultDraft.defaultReasoning || "__none__"}
                      onValueChange={(value) => {
                        const next = String(value || "");
                        setDefaultDraft((current) => ({
                          ...current,
                          defaultReasoning: next === "__none__" ? "" : next,
                        }));
                      }}
                    >
                      <SelectTrigger className="w-full">
                        <SelectValue placeholder="跟随模型默认" />
                      </SelectTrigger>
                      <SelectContent>
                        {REASONING_OPTIONS.map((item) => (
                          <SelectItem key={item.value} value={item.value}>
                            {item.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <div className="flex items-center justify-end gap-2">
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-7 px-2 text-[11px]"
                        onClick={() =>
                          setDefaultDraft((current) => ({
                            ...current,
                            defaultReasoning: "",
                          }))
                        }
                        disabled={!defaultDraft.defaultReasoning.trim()}
                      >
                        清空 reasoning
                      </Button>
                    </div>
                  </div>
                  <div className="space-y-2 rounded-lg border bg-background/30 p-3">
                    <div className="flex items-center justify-between gap-3">
                      <span className="text-xs text-muted-foreground">
                        新 session 继承同 workspace 上次模型
                      </span>
                      <Switch
                        checked={defaultDraft.inheritLastSession}
                        onCheckedChange={(checked) =>
                          setDefaultDraft((current) => ({
                            ...current,
                            inheritLastSession: checked,
                          }))
                        }
                      />
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <span className="text-xs text-muted-foreground">
                        session 自动记忆
                      </span>
                      <Switch
                        checked={defaultDraft.autoRemember}
                        onCheckedChange={(checked) =>
                          setDefaultDraft((current) => ({
                            ...current,
                            autoRemember: checked,
                          }))
                        }
                      />
                    </div>
                  </div>
                  <div className="rounded-lg border border-sky-500/30 bg-sky-500/10 p-3 text-[11px] text-sky-700">
                    新建 Codex session / thread 后，如果它还没出现在右侧“当前会话模型”里，先点这里把当前默认模型直接写到该 workspace 最新主线程。这个动作只命中最新主线程，不会改子 Agent thread。
                  </div>
                  <Button
                    variant="outline"
                    className="w-full gap-2"
                    onClick={applyDraftToLatestMainThread}
                    disabled={
                      !defaultDraft.workspace.trim() ||
                      defaultDraft.workspace.trim() === "__global__" ||
                      !defaultDraft.defaultModel.trim() ||
                      applyLatestWorkspaceSessionMutation.isPending
                    }
                  >
                    <Zap className="h-4 w-4" />
                    应用到该 workspace 最新主线程
                  </Button>
                  <Button
                    className="w-full gap-2"
                    onClick={() => setDefaultMutation.mutate()}
                    disabled={!defaultDraft.workspace.trim() || setDefaultMutation.isPending}
                  >
                    <Save className="h-4 w-4" />
                    保存默认策略
                  </Button>
                  <Button
                    variant="ghost"
                    className="w-full gap-2"
                    onClick={() =>
                      deleteDefaultMutation.mutate(defaultDraft.workspace.trim() || "__global__")
                    }
                    disabled={
                      !defaultDraft.workspace.trim() || deleteDefaultMutation.isPending
                    }
                  >
                    <Database className="h-4 w-4" />
                    清除默认策略
                  </Button>
                </CardContent>
              </Card>

              <Card className="glass-card border border-border shadow-md backdrop-blur-md">
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <Database className="h-4 w-4" /> 已保存默认项
                  </CardTitle>
                </CardHeader>
                <CardContent className="overflow-x-auto">
                  <Table>
                    <TableHeader>
                    <TableRow>
                        <TableHead>项目/工作区</TableHead>
                        <TableHead>默认模型</TableHead>
                        <TableHead>reasoning</TableHead>
                        <TableHead>继承上次</TableHead>
                        <TableHead>自动记忆</TableHead>
                        <TableHead>更新时间</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {(sessionsQuery.data?.workspaceDefaults ?? []).length === 0 ? (
                      <TableRow>
                          <TableCell colSpan={6} className="h-24 text-center text-muted-foreground">
                            尚未保存 workspace 默认模型
                          </TableCell>
                        </TableRow>
                      ) : (
                        (sessionsQuery.data?.workspaceDefaults ?? []).map((item) => (
                        <TableRow
                            key={item.workspace}
                            className="cursor-pointer"
                            onClick={() =>
                              setDefaultDraft({
                                workspace: item.workspace,
                                defaultModel: item.defaultModel || "",
                                defaultReasoning: item.defaultReasoningEffort || "",
                                inheritLastSession: item.inheritLastSession,
                                autoRemember: item.autoRemember,
                              })
                            }
                          >
                            <TableCell className="max-w-[360px] truncate font-mono text-xs">
                              {item.workspace}
                            </TableCell>
                            <TableCell className="font-mono text-xs">
                              {item.defaultModel || "-"}
                            </TableCell>
                            <TableCell>{item.defaultReasoningEffort || "-"}</TableCell>
                            <TableCell>{item.inheritLastSession ? "是" : "否"}</TableCell>
                            <TableCell>{item.autoRemember ? "是" : "否"}</TableCell>
                            <TableCell className="text-xs text-muted-foreground">
                              {formatTsFromSeconds(item.updatedAt)}
                            </TableCell>
                          </TableRow>
                        ))
                      )}
                    </TableBody>
                  </Table>
                </CardContent>
              </Card>
            </div>
          </TabsContent>

          <TabsContent value="logs" className="space-y-4">
            <div className="grid gap-4 xl:grid-cols-2">
              <Card className="glass-card border border-border shadow-md backdrop-blur-md">
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <Activity className="h-4 w-4" /> 真实路由命中
                  </CardTitle>
                </CardHeader>
                <CardContent className="space-y-2">
                  {routeRequestLogs.length === 0 ? (
                    <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
                      暂无真实路由日志。请求经过聚合 API 或内置转换后会显示在这里。
                    </div>
                  ) : (
                    routeRequestLogs.map((item: RequestLog, index: number) => (
                      <div
                        key={`${item.traceId || item.createdAt || "log"}-${index}`}
                        className="rounded-lg border bg-background/35 p-3"
                      >
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-mono text-xs">{item.model || "未记录模型"}</span>
                          {item.statusCode && item.statusCode >= 400 ? (
                            <Badge className="border-red-500/20 bg-red-500/10 text-red-500">
                              {item.statusCode}
                            </Badge>
                          ) : (
                            <Badge className="border-emerald-500/20 bg-emerald-500/10 text-emerald-500">
                              {item.statusCode || 200}
                            </Badge>
                          )}
                          {item.responseAdapter ? (
                            <Badge variant="secondary">{item.responseAdapter}</Badge>
                          ) : null}
                        </div>
                        <div className="mt-2 grid gap-1 text-[11px] text-muted-foreground">
                          <span>
                            path: {item.originalPath || item.requestPath}
                            {item.adaptedPath && item.adaptedPath !== item.requestPath
                              ? ` -> ${item.adaptedPath}`
                              : ""}
                          </span>
                          <span>
                            上游:{" "}
                            {item.aggregateApiSupplierName ||
                              item.aggregateApiUrl ||
                              item.upstreamUrl ||
                              "-"}
                          </span>
                          <span>
                            尝试链:{" "}
                            {item.attemptedAggregateApiIds.length > 0
                              ? item.attemptedAggregateApiIds.join(" -> ")
                              : "-"}
                          </span>
                          {item.error ? (
                            <span className="text-red-500">错误: {item.error}</span>
                          ) : null}
                          <span
                            title={
                              item.conversationId
                                ? `项目: ${
                                    requestLogContextByConversationId.get(item.conversationId)
                                      ?.workspace || "-"
                                  } / 会话: ${item.conversationId}`
                                : "未记录会话"
                            }
                          >
                            {formatTsFromSeconds(item.createdAt || 0)}
                          </span>
                        </div>
                      </div>
                    ))
                  )}
                </CardContent>
              </Card>

              <Card className="glass-card border border-border shadow-md backdrop-blur-md">
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <ShieldAlert className="h-4 w-4" /> 路由绑定状态
                  </CardTitle>
                </CardHeader>
                <CardContent className="space-y-2">
                  {bindings.filter((item) => item.lastError || item.requiresAdapter || item.lastSuccessAt).length === 0 ? (
                    <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
                      暂无路由运行状态
                    </div>
                  ) : (
                    bindings
                      .filter((item) => item.lastError || item.requiresAdapter || item.lastSuccessAt)
                      .slice(0, 8)
                      .map((item) => (
                        <div key={item.id} className="rounded-lg border bg-background/35 p-3">
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="font-mono text-xs">{item.model}</span>
                            <Badge variant="secondary">
                              {item.aggregateApiName || item.aggregateApiId}
                            </Badge>
                            {statusBadge(item.lastProbeStatus)}
                            {item.requiresAdapter ? (
                              <Badge className="border-amber-500/20 bg-amber-500/10 text-amber-500">
                                responses 转 chat
                              </Badge>
                            ) : null}
                          </div>
                          <p className="mt-2 text-xs text-muted-foreground">
                            {item.lastError ||
                              (item.lastSuccessAt
                                ? `最近成功：${formatTsFromSeconds(item.lastSuccessAt)}`
                                : "该绑定会在网关内执行协议转换，失败会进入 request_logs 与 gateway_error_logs。")}
                          </p>
                        </div>
                      ))
                  )}
                </CardContent>
              </Card>

              <Card className="glass-card border border-border shadow-md backdrop-blur-md">
                <CardHeader>
                  <CardTitle className="flex items-center gap-2">
                    <Activity className="h-4 w-4" /> 最近探测日志
                  </CardTitle>
                </CardHeader>
                <CardContent className="space-y-2">
                  {latestProbeRunsByApi.length === 0 ? (
                    <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
                      暂无探测日志
                    </div>
                  ) : (
                    latestProbeRunsByApi.slice(0, 8).map((run: ProbeRunSummary) => (
                      <div key={run.id} className="rounded-lg border border-border bg-background/45 p-3">
                        <div className="flex items-center justify-between gap-2">
                          <div className="min-w-0">
                            <div className="truncate text-sm font-medium">
                              {run.aggregateApiName || run.aggregateApiId}
                            </div>
                            <div className="mt-0.5 text-[11px] text-muted-foreground">
                              {formatTsFromSeconds(run.finishedAt || run.startedAt)}
                            </div>
                          </div>
                          {statusBadge(run.status)}
                        </div>
                        <div className="mt-2 grid gap-1 text-[11px] text-muted-foreground">
                          <span>models: {run.modelsStatus || "-"}</span>
                          <span>responses: {run.responsesStatus || "-"}</span>
                          <span>chat: {run.chatCompletionsStatus || "-"}</span>
                        </div>
                      </div>
                    ))
                  )}
                </CardContent>
              </Card>
            </div>
          </TabsContent>
        </Tabs>
      )}
    </div>
  );
}






