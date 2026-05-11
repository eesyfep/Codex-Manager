"use client";

import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent,
} from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowUp,
  Boxes,
  ChevronDown,
  ChevronRight,
  Copy,
  Eye,
  EyeOff,
  MoreVertical,
  Plus,
  RefreshCw,
  Settings2,
  ShieldCheck,
  Sparkles,
  Timer,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { AggregateApiModal } from "@/components/modals/aggregate-api-modal";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { NumberStepper } from "@/components/ui/number-stepper";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { accountClient } from "@/lib/api/account-client";
import { modelRouterClient } from "@/lib/api/model-router";
import { copyTextToClipboard } from "@/lib/utils/clipboard";
import { cn } from "@/lib/utils";
import { formatTsFromSeconds } from "@/lib/utils/usage";
import { useAppStore } from "@/lib/store/useAppStore";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useI18n } from "@/lib/i18n/provider";
import { AggregateApi, AggregateApiModelUsage, AggregateApiSecretResult } from "@/types";
import type { ModelRouteBindingSummary, ProbeCandidateSummary } from "@/types/model-router";

interface ApiModelLatencyState {
  status: "idle" | "testing" | "success" | "failed";
  testedAt: number | null;
  successCount: number;
  failCount: number;
  fastestMs: number | null;
  testedModelCount: number;
  totalModelCount: number;
  testedAllModels: boolean;
  models: Array<{
    model: string;
    ok: boolean;
    latencyMs: number | null;
    error: string | null;
  }>;
}

interface AggregateApiUrlGroup {
  key: string;
  providerType: string;
  url: string;
  items: AggregateApi[];
  fastestMs: number | null;
  successCount: number;
  failCount: number;
  minSort: number;
  hasError: boolean;
  modelUsages: AggregateApiModelUsage[];
  usageTotalTokens: number;
  usageTotalCostUsd: number;
  usageRequestCount: number;
}

interface BindingDialogState {
  apiId: string;
  apiIds: string[];
  modelDraft: string;
  selectedCandidateIds: string[];
  probeCandidates: ProbeCandidateSummary[];
  probing: boolean;
}

type TranslateFn = (key: string, values?: Record<string, string | number>) => string;
type AggregateApiPoolFilter = "primary" | "wool";
type AggregateApiTableColumn =
  | "select"
  | "provider"
  | "secret"
  | "sort"
  | "test"
  | "latency"
  | "status"
  | "action";

const AGGREGATE_API_PROVIDER_LABELS: Record<string, string> = {
  codex: "Codex",
  claude: "Claude",
};

const AGGREGATE_API_PROVIDER_FILTER_LABELS: Record<string, string> = {
  all: "全部类型",
  codex: "Codex",
  claude: "Claude",
};

type AggregateApiTableColumnWidths = Record<AggregateApiTableColumn, number>;

const AGGREGATE_API_TABLE_DEFAULT_WIDTHS: AggregateApiTableColumnWidths = {
  select: 46,
  provider: 500,
  secret: 150,
  sort: 72,
  test: 150,
  latency: 170,
  status: 112,
  action: 290,
};

const AGGREGATE_API_TABLE_MIN_WIDTHS: AggregateApiTableColumnWidths = {
  select: 46,
  provider: 380,
  secret: 130,
  sort: 64,
  test: 132,
  latency: 150,
  status: 96,
  action: 250,
};

const AGGREGATE_API_TABLE_WIDTH_STORAGE_KEY = "codexmanager.aggregateApi.tableColumnWidths.v4";
const AGGREGATE_API_TABLE_WIDTH_PROFILE_STORAGE_KEY =
  "codexmanager.aggregateApi.tableColumnWidthProfiles.v4";

interface AggregateApiTableWidthProfile {
  id: string;
  name: string;
  bucket: string;
  widths: AggregateApiTableColumnWidths;
  updatedAt: number;
}

function tableWidthStorageKey(): string {
  if (typeof window === "undefined") return AGGREGATE_API_TABLE_WIDTH_STORAGE_KEY;
  const width = window.innerWidth || 0;
  const bucket = width >= 1500 ? "wide" : width >= 1180 ? "desktop" : "compact";
  return `${AGGREGATE_API_TABLE_WIDTH_STORAGE_KEY}.${bucket}`;
}

function tableWidthBucket(): string {
  if (typeof window === "undefined") return "desktop";
  const width = window.innerWidth || 0;
  return width >= 1500 ? "wide" : width >= 1180 ? "desktop" : "compact";
}

function tableWidthBucketLabel(bucket: string): string {
  if (bucket === "wide") return "全屏/宽屏列宽";
  if (bucket === "compact") return "窄窗口列宽";
  return "桌面窗口列宽";
}

function tableWidthProfilesStorageKey(): string {
  if (typeof window === "undefined") return AGGREGATE_API_TABLE_WIDTH_PROFILE_STORAGE_KEY;
  return `${AGGREGATE_API_TABLE_WIDTH_PROFILE_STORAGE_KEY}.${tableWidthBucket()}`;
}

function groupRowBackground(hasChildren: boolean, expanded: boolean): string {
  if (expanded) return "bg-slate-200/75 dark:bg-slate-900/75";
  return hasChildren ? "bg-slate-100/70 dark:bg-slate-900/55" : "bg-slate-100/75 dark:bg-slate-900/60";
}

function keyRowBackground(): string {
  return "bg-slate-50/90 dark:bg-slate-950/35";
}

function formatCompactNumber(value: number): string {
  if (!Number.isFinite(value)) return "0";
  return new Intl.NumberFormat("zh-CN", { notation: "compact" }).format(value);
}

function formatUsd(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "$0";
  if (value < 0.01) return `$${value.toFixed(4)}`;
  return `$${value.toFixed(2)}`;
}

function readStoredTableColumnWidths(): AggregateApiTableColumnWidths | null {
  if (typeof window === "undefined") return null;
  try {
    const parsed = JSON.parse(
      window.localStorage.getItem(tableWidthStorageKey()) ||
        window.localStorage.getItem(AGGREGATE_API_TABLE_WIDTH_STORAGE_KEY) ||
        "null"
    );
    return normalizeTableColumnWidths(parsed);
  } catch {
    return null;
  }
}

function normalizeTableColumnWidths(input: unknown): AggregateApiTableColumnWidths | null {
  if (!input || typeof input !== "object") return null;
  return (Object.keys(AGGREGATE_API_TABLE_DEFAULT_WIDTHS) as AggregateApiTableColumn[]).reduce(
    (result, key) => {
      const raw = Number((input as Record<string, unknown>)[key]);
      result[key] = Number.isFinite(raw)
        ? Math.max(AGGREGATE_API_TABLE_MIN_WIDTHS[key], Math.round(raw))
        : AGGREGATE_API_TABLE_DEFAULT_WIDTHS[key];
      return result;
    },
    {} as AggregateApiTableColumnWidths,
  );
}

function readStoredTableColumnWidthProfiles(): AggregateApiTableWidthProfile[] {
  if (typeof window === "undefined") return [];
  try {
    const parsed = JSON.parse(window.localStorage.getItem(tableWidthProfilesStorageKey()) || "[]");
    if (!Array.isArray(parsed)) return [];
    return parsed
      .map((item) => {
        if (!item || typeof item !== "object") return null;
        const widths = normalizeTableColumnWidths((item as Record<string, unknown>).widths);
        if (!widths) return null;
        return {
          id: String((item as Record<string, unknown>).id || ""),
          name: String((item as Record<string, unknown>).name || ""),
          bucket: String((item as Record<string, unknown>).bucket || tableWidthBucket()),
          widths,
          updatedAt: Number((item as Record<string, unknown>).updatedAt || 0),
        } satisfies AggregateApiTableWidthProfile;
      })
      .filter((item): item is AggregateApiTableWidthProfile => Boolean(item?.id && item.name));
  } catch {
    return [];
  }
}

function writeStoredTableColumnWidthProfiles(profiles: AggregateApiTableWidthProfile[]) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(tableWidthProfilesStorageKey(), JSON.stringify(profiles));
}

function tableColumnStyle(widths: AggregateApiTableColumnWidths) {
  const columns = [
    widths.select,
    widths.provider,
    widths.secret,
    widths.sort,
    widths.test,
    widths.latency,
    widths.status,
    widths.action,
  ];
  return {
    gridTemplateColumns: columns.map((width) => `${width}px`).join(" "),
    minWidth: columns.reduce((total, width) => total + width, 0),
  };
}

function HeaderResizeHandle({
  column,
  onResizeStart,
}: {
  column: AggregateApiTableColumn;
  onResizeStart: (
    column: AggregateApiTableColumn,
    event: PointerEvent<HTMLButtonElement>,
  ) => void;
}) {
  return (
    <button
      type="button"
      aria-label="拖动调整列宽"
      className="absolute right-0 top-0 h-full w-3 cursor-col-resize touch-none border-r border-border/80 bg-border/40 transition-colors before:absolute before:right-1 before:top-1/2 before:h-8 before:w-px before:-translate-y-1/2 before:bg-foreground/45 hover:border-primary hover:bg-primary/35 hover:before:bg-primary"
      onPointerDown={(event) => onResizeStart(column, event)}
    />
  );
}

async function runWithConcurrency<T, R>(
  items: T[],
  concurrency: number,
  worker: (item: T, index: number) => Promise<R>,
  shouldStop?: () => boolean,
): Promise<R[]> {
  const results: R[] = new Array(items.length);
  let nextIndex = 0;
  const runners = Array.from({ length: Math.min(concurrency, items.length) }, async () => {
    while (nextIndex < items.length) {
      if (shouldStop?.()) {
        break;
      }
      const index = nextIndex;
      nextIndex += 1;
      if (shouldStop?.()) {
        break;
      }
      results[index] = await worker(items[index], index);
    }
  });
  await Promise.all(runners);
  return results.filter((item): item is R => item !== undefined);
}

/**
 * 函数 `getTestBadge`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - api: 参数 api
 *
 * # 返回
 * 返回函数执行结果
 */
function getTestBadge(api: AggregateApi, t: TranslateFn) {
  if (api.lastTestStatus === "success") {
    return (
      <Badge className="border-green-500/20 bg-green-500/10 text-green-500">
        {t("已连通")}
      </Badge>
    );
  }
  if (api.lastTestStatus === "failed") {
    return (
      <Badge className="border-red-500/20 bg-red-500/10 text-red-500">
        {t("失败")}
      </Badge>
    );
  }
  return <Badge variant="secondary">{t("未测试")}</Badge>;
}

function appendUniqueModels(target: string[], source: Iterable<string | null | undefined>) {
  for (const item of source) {
    const model = String(item || "").trim();
    if (!model) continue;
    if (!target.some((existing) => existing.toLowerCase() === model.toLowerCase())) {
      target.push(model);
    }
  }
}

function splitModelCandidates(raw: string | null | undefined): string[] {
  return String(raw || "")
    .split(/[\n\r,;]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

export default function AggregateApiPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const appSettings = useAppStore((state) => state.appSettings);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const isPageActive = useDesktopPageActive("/aggregate-api/");
  const isQueryEnabled = useDeferredDesktopActivation(
    isServiceReady && isPageActive,
  );
  const [modalOpen, setModalOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [templateApiId, setTemplateApiId] = useState<string | null>(null);
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const [providerFilter, setProviderFilter] = useState("all");
  const [poolFilter, setPoolFilter] = useState<AggregateApiPoolFilter>("primary");
  const [tableWidthMode, setTableWidthMode] = useState(tableWidthBucket);
  const [tableColumnWidths, setTableColumnWidths] =
    useState<AggregateApiTableColumnWidths>(AGGREGATE_API_TABLE_DEFAULT_WIDTHS);
  const [tableWidthProfiles, setTableWidthProfiles] = useState<
    AggregateApiTableWidthProfile[]
  >([]);
  const [selectedTableWidthProfileId, setSelectedTableWidthProfileId] =
    useState<string>("");
  const [revealedSecrets, setRevealedSecrets] = useState<
    Record<string, AggregateApiSecretResult>
  >({});
  const [loadingSecretId, setLoadingSecretId] = useState<string | null>(null);
  const [testingApiId, setTestingApiId] = useState<string | null>(null);
  const [testingAll, setTestingAll] = useState(false);
  const [testingLatencyAll, setTestingLatencyAll] = useState(false);
  const [latencyTestingApiIds, setLatencyTestingApiIds] = useState<string[]>([]);
  const latencyAbortControllersRef = useRef<Record<string, AbortController>>({});
  const latencyBatchCanceledRef = useRef(false);
  const [selectedApiIds, setSelectedApiIds] = useState<string[]>([]);
  const [expandedGroupKeys, setExpandedGroupKeys] = useState<string[]>([]);
  const initializedGroupKeysRef = useRef<Set<string>>(new Set());
  const [confirmBulkDeleteOpen, setConfirmBulkDeleteOpen] = useState(false);
  const [bulkOperation, setBulkOperation] = useState<"enable" | "disable" | "delete" | null>(null);
  const [bindingDialog, setBindingDialog] = useState<BindingDialogState | null>(null);
  const [apiModelLatencies, setApiModelLatencies] = useState<
    Record<string, ApiModelLatencyState>
  >({});
  const [togglingApiId, setTogglingApiId] = useState<string | null>(null);
  const [statusOverrides, setStatusOverrides] = useState<Record<string, boolean>>(
    {},
  );
  const tableGridStyle = tableColumnStyle(tableColumnWidths);
  const tablePixelWidth = tableGridStyle.minWidth;

  const { data: aggregateApis = [], isLoading } = useQuery({
    queryKey: ["aggregate-apis"],
    queryFn: () => accountClient.listAggregateApis(),
    enabled: isQueryEnabled,
    retry: 1,
  });
  const { data: aggregateApiModelUsages = [] } = useQuery({
    queryKey: ["aggregate-api-model-usage"],
    queryFn: () => accountClient.listAggregateApiModelUsage(),
    enabled: isQueryEnabled,
    retry: 1,
    staleTime: 30_000,
  });

  usePageTransitionReady("/aggregate-api/", !isServiceReady || !isLoading);

  useEffect(() => {
    if (isPageActive) return;
    setModalOpen(false);
    setEditingId(null);
    setDeleteId(null);
  }, [isPageActive]);

  useEffect(() => {
    const stored = readStoredTableColumnWidths();
    if (stored) {
      setTableColumnWidths(stored);
    }
    const profiles = readStoredTableColumnWidthProfiles();
    setTableWidthProfiles(profiles);
    setSelectedTableWidthProfileId(profiles[0]?.id || "");
  }, []);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const handleResize = () => {
      const nextMode = tableWidthBucket();
      setTableWidthMode((current) => {
        if (current === nextMode) return current;
        setTableColumnWidths(readStoredTableColumnWidths() ?? AGGREGATE_API_TABLE_DEFAULT_WIDTHS);
        const profiles = readStoredTableColumnWidthProfiles();
        setTableWidthProfiles(profiles);
        setSelectedTableWidthProfileId(profiles[0]?.id || "");
        return nextMode;
      });
    };
    window.addEventListener("resize", handleResize);
    return () => window.removeEventListener("resize", handleResize);
  }, []);

  const resizeTableColumn = (
    column: AggregateApiTableColumn,
    event: PointerEvent<HTMLButtonElement>,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    const startX = event.clientX;
    const startWidth = tableColumnWidths[column];
    const minWidth = AGGREGATE_API_TABLE_MIN_WIDTHS[column];

    const readClientX = (input: Event) =>
      "clientX" in input ? Number((input as globalThis.PointerEvent).clientX) : startX;
    const handleMove = (moveEvent: Event) => {
      const nextWidth = Math.max(
        minWidth,
        Math.round(startWidth + readClientX(moveEvent) - startX),
      );
      setTableColumnWidths((current) => ({
        ...current,
        [column]: nextWidth,
      }));
    };

    const handleUp = (upEvent: Event) => {
      const nextWidth = Math.max(
        minWidth,
        Math.round(startWidth + readClientX(upEvent) - startX),
      );
      const next = {
        ...tableColumnWidths,
        [column]: nextWidth,
      };
      setTableColumnWidths(next);
      if (typeof window !== "undefined") {
        window.localStorage.setItem(
          tableWidthStorageKey(),
          JSON.stringify(next),
        );
      }
      toast.success(`${tableWidthBucketLabel(tableWidthBucket())}已保存`);
      window.removeEventListener("pointermove", handleMove);
      window.removeEventListener("pointerup", handleUp);
    };

    window.addEventListener("pointermove", handleMove);
    window.addEventListener("pointerup", handleUp);
  };

  const resetTableColumnWidths = () => {
    const next = AGGREGATE_API_TABLE_DEFAULT_WIDTHS;
    setTableColumnWidths(next);
    if (typeof window !== "undefined") {
      window.localStorage.setItem(tableWidthStorageKey(), JSON.stringify(next));
    }
    toast.success(`${tableWidthBucketLabel(tableWidthBucket())}已重置`);
  };

  const applySavedTableColumnWidths = () => {
    const profile = tableWidthProfiles.find((item) => item.id === selectedTableWidthProfileId);
    const stored = profile?.widths ?? readStoredTableColumnWidths();
    if (!stored) {
      toast.info(`当前没有已保存的${tableWidthBucketLabel(tableWidthBucket())}`);
      return;
    }
    setTableColumnWidths(stored);
    toast.success(`${profile?.name || tableWidthBucketLabel(tableWidthBucket())}已应用`);
  };

  const saveTableColumnWidths = () => {
    const defaultName = `${tableWidthBucketLabel(tableWidthBucket())} ${new Date().toLocaleString("zh-CN", {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    })}`;
    const rawName = typeof window !== "undefined"
      ? window.prompt("给这套列宽命名", defaultName)
      : defaultName;
    const name = String(rawName || "").trim();
    if (!name) {
      toast.info("已取消保存列宽");
      return;
    }
    const profile: AggregateApiTableWidthProfile = {
      id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      name,
      bucket: tableWidthBucket(),
      widths: tableColumnWidths,
      updatedAt: Date.now(),
    };
    const nextProfiles = [profile, ...tableWidthProfiles].slice(0, 12);
    setTableWidthProfiles(nextProfiles);
    setSelectedTableWidthProfileId(profile.id);
    writeStoredTableColumnWidthProfiles(nextProfiles);
    if (typeof window !== "undefined") {
      window.localStorage.setItem(tableWidthStorageKey(), JSON.stringify(tableColumnWidths));
    }
    toast.success(`列宽方案“${name}”已保存`);
  };

  const deleteSelectedTableWidthProfile = () => {
    if (!selectedTableWidthProfileId) {
      toast.info("请先选择列宽方案");
      return;
    }
    const profile = tableWidthProfiles.find((item) => item.id === selectedTableWidthProfileId);
    const nextProfiles = tableWidthProfiles.filter((item) => item.id !== selectedTableWidthProfileId);
    setTableWidthProfiles(nextProfiles);
    setSelectedTableWidthProfileId(nextProfiles[0]?.id || "");
    writeStoredTableColumnWidthProfiles(nextProfiles);
    toast.success(`列宽方案“${profile?.name || selectedTableWidthProfileId}”已删除`);
  };

  useEffect(() => {
    setStatusOverrides((current) => {
      const serverStatusMap = new Map(
        aggregateApis.map((item) => [
          item.id,
          String(item.status || "").trim().toLowerCase() !== "disabled",
        ]),
      );
      let changed = false;
      const next: Record<string, boolean> = {};

      Object.entries(current).forEach(([id, enabled]) => {
        const serverEnabled = serverStatusMap.get(id);
        if (serverEnabled == null) {
          changed = true;
          return;
        }
        if (serverEnabled !== enabled) {
          next[id] = enabled;
          return;
        }
        changed = true;
      });

      return changed ? next : current;
    });
  }, [aggregateApis]);

  useEffect(() => {
    const currentIds = new Set(
      aggregateApis
        .filter((api) => (api.pool === "wool" ? "wool" : "primary") === poolFilter)
        .map((api) => api.id),
    );
    setSelectedApiIds((current) => current.filter((id) => currentIds.has(id)));
  }, [aggregateApis, poolFilter]);

  const editingApi = useMemo(
    () => aggregateApis.find((item) => item.id === editingId) || null,
    [aggregateApis, editingId],
  );
  const templateApi = useMemo(
    () => aggregateApis.find((item) => item.id === templateApiId) || null,
    [aggregateApis, templateApiId],
  );

  const filteredAggregateApis = useMemo(() => {
    const poolApis = aggregateApis.filter(
      (api) => (api.pool === "wool" ? "wool" : "primary") === poolFilter,
    );
    if (providerFilter === "all") {
      return poolApis;
    }
    return poolApis.filter((api) => api.providerType === providerFilter);
  }, [aggregateApis, poolFilter, providerFilter]);

  const aggregateApiById = useMemo(
    () => new Map(aggregateApis.map((api) => [api.id, api])),
    [aggregateApis],
  );
  const selectedApis = useMemo(
    () =>
      selectedApiIds
        .map((id) => aggregateApiById.get(id))
        .filter((api): api is AggregateApi => Boolean(api)),
    [aggregateApiById, selectedApiIds],
  );
  const filteredApiIds = useMemo(
    () => filteredAggregateApis.map((api) => api.id),
    [filteredAggregateApis],
  );
  const allFilteredSelected =
    filteredApiIds.length > 0 && filteredApiIds.every((id) => selectedApiIds.includes(id));
  const someFilteredSelected = filteredApiIds.some((id) => selectedApiIds.includes(id));
  const latencyTargetApis = selectedApis.length > 0 ? selectedApis : filteredAggregateApis;

  const modelUsagesByUrl = useMemo(() => {
    const map = new Map<string, AggregateApiModelUsage[]>();
    for (const usage of aggregateApiModelUsages) {
      const key = normalizeAggregateApiUrl(usage.aggregateApiUrl);
      if (!key) continue;
      const list = map.get(key) ?? [];
      list.push(usage);
      map.set(key, list);
    }
    return map;
  }, [aggregateApiModelUsages]);

  const aggregateApiGroups = useMemo(() => {
    const map = new Map<string, AggregateApiUrlGroup>();
    for (const api of filteredAggregateApis) {
      const urlKey = normalizeAggregateApiUrl(api.url) || api.id;
      const key = `${api.providerType || "codex"}|${urlKey}`;
      const latency = apiModelLatencies[api.id];
      const existing =
        map.get(key) ||
        ({
          key,
          providerType: api.providerType || "codex",
          url: api.url,
          items: [],
          fastestMs: null,
          successCount: 0,
          failCount: 0,
          minSort: Number(api.sort) || 0,
          hasError: false,
          modelUsages: [],
          usageTotalTokens: 0,
          usageTotalCostUsd: 0,
          usageRequestCount: 0,
        } satisfies AggregateApiUrlGroup);
      existing.items.push(api);
      existing.minSort = Math.min(existing.minSort, Number(api.sort) || 0);
      existing.hasError =
        existing.hasError ||
        api.lastTestStatus === "failed" ||
        latency?.status === "failed";
      if (latency?.status === "success") {
        existing.successCount += 1;
        if (latency.fastestMs != null) {
          existing.fastestMs =
            existing.fastestMs == null
              ? latency.fastestMs
              : Math.min(existing.fastestMs, latency.fastestMs);
        }
      } else if (latency?.status === "failed" || api.lastTestStatus === "failed") {
        existing.failCount += 1;
      }
      map.set(key, existing);
    }
    return Array.from(map.values())
      .map((group) => ({
        ...group,
        items: group.items.sort((left, right) => left.sort - right.sort || left.id.localeCompare(right.id)),
        modelUsages: (modelUsagesByUrl.get(normalizeAggregateApiUrl(group.url)) ?? [])
          .slice()
          .sort(
            (left, right) =>
              right.totalTokens - left.totalTokens ||
              right.requestCount - left.requestCount ||
              left.model.localeCompare(right.model),
          ),
        usageTotalTokens: (modelUsagesByUrl.get(normalizeAggregateApiUrl(group.url)) ?? [])
          .reduce((sum, item) => sum + item.totalTokens, 0),
        usageTotalCostUsd: (modelUsagesByUrl.get(normalizeAggregateApiUrl(group.url)) ?? [])
          .reduce((sum, item) => sum + item.estimatedCostUsd, 0),
        usageRequestCount: (modelUsagesByUrl.get(normalizeAggregateApiUrl(group.url)) ?? [])
          .reduce((sum, item) => sum + item.requestCount, 0),
      }))
      .sort((left, right) => left.minSort - right.minSort || left.url.localeCompare(right.url));
  }, [apiModelLatencies, filteredAggregateApis, modelUsagesByUrl]);

  const routeBindingsQuery = useQuery({
    queryKey: ["model-router", "bindings"],
    queryFn: () => modelRouterClient.listBindings(),
    enabled: isQueryEnabled,
    retry: 1,
  });

  const routeBindingsByAggregateApiId = useMemo(() => {
    const map = new Map<string, ModelRouteBindingSummary[]>();
    for (const binding of routeBindingsQuery.data?.items ?? []) {
      const list = map.get(binding.aggregateApiId) ?? [];
      list.push(binding);
      map.set(binding.aggregateApiId, list);
    }
    return map;
  }, [routeBindingsQuery.data?.items]);

  const knownRouteModels = useMemo(() => {
    const values = new Set<string>();
    for (const binding of routeBindingsQuery.data?.items ?? []) {
      if (binding.model) values.add(binding.model);
    }
    for (const usage of aggregateApiModelUsages) {
      if (usage.model) values.add(usage.model);
    }
    return Array.from(values).sort((left, right) => left.localeCompare(right));
  }, [aggregateApiModelUsages, routeBindingsQuery.data?.items]);

  const buildLatencyTestModels = (
    api: AggregateApi,
    probeCandidates: string[],
  ): { models: string[]; source: string } => {
    const models: string[] = [];
    appendUniqueModels(models, probeCandidates);
    if (models.length > 0) {
      return { models, source: "/v1/models" };
    }

    const bindingModels = Array.from(
      new Set((routeBindingsByAggregateApiId.get(api.id) ?? []).map((binding) => binding.model)),
    );
    appendUniqueModels(models, bindingModels);
    if (models.length > 0) {
      return { models, source: "绑定模型" };
    }

    appendUniqueModels(models, splitModelCandidates(appSettings.modelRouterProbeFallbackModels));
    return { models, source: "测响兜底模型" };
  };

  const cancelLatencyTests = () => {
    latencyBatchCanceledRef.current = true;
    for (const controller of Object.values(latencyAbortControllersRef.current)) {
      controller.abort();
    }
    latencyAbortControllersRef.current = {};
    setTestingLatencyAll(false);
    toast.info("已取消测响，当前请求完成后不会继续后续模型");
  };

  const defaultCreateSort = useMemo(() => {
    const maxSort = filteredAggregateApis.reduce(
      (max, api) => Math.max(max, Number(api.sort) || 0),
      0,
    );
    return maxSort + 5;
  }, [filteredAggregateApis]);

  const defaultWoolSupplierName = useMemo(() => {
    const maxIndex = aggregateApis.reduce((max, api) => {
      if (api.pool !== "wool") return max;
      const match = String(api.supplierName || "").trim().match(/^羊毛(\d+)$/);
      if (!match) return max;
      return Math.max(max, Number(match[1]) || 0);
    }, 0);
    return `羊毛${maxIndex + 1}`;
  }, [aggregateApis]);

  useEffect(() => {
    const knownKeys = new Set(aggregateApiGroups.map((group) => group.key));
    const initialized = initializedGroupKeysRef.current;
    for (const key of Array.from(initialized)) {
      if (!knownKeys.has(key)) {
        initialized.delete(key);
      }
    }
    const newlyInitialized = aggregateApiGroups.filter(
      (group) => !initialized.has(group.key),
    );
    newlyInitialized.forEach((group) => initialized.add(group.key));
    setExpandedGroupKeys((current) => {
      const next = new Set(current.filter((key) => knownKeys.has(key)));
      newlyInitialized.forEach((group) => {
        if (group.hasError || group.items.length === 1) {
          next.add(group.key);
        }
      });
      const nextList = Array.from(next);
      return nextList.length === current.length &&
        nextList.every((key, index) => key === current[index])
        ? current
        : nextList;
    });
  }, [aggregateApiGroups]);

  /**
   * 函数 `renderTestStatus`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - api: 参数 api
   *
   * # 返回
   * 返回函数执行结果
   */
  const renderTestStatus = (api: AggregateApi) => {
    const badge = getTestBadge(api, t);
    if (api.lastTestStatus !== "failed" || !api.lastTestError) {
      return badge;
    }

    return (
      <Tooltip>
        <TooltipTrigger render={<div />} className="inline-flex cursor-help">
          {badge}
        </TooltipTrigger>
        <TooltipContent className="max-w-sm whitespace-pre-wrap break-words">
          {api.lastTestError}
        </TooltipContent>
      </Tooltip>
    );
  };

  const runApiModelLatencyTest = async (
    api: AggregateApi,
    forceAllModels = false,
  ) => {
    const previousController = latencyAbortControllersRef.current[api.id];
    previousController?.abort();
    const controller = new AbortController();
    latencyAbortControllersRef.current[api.id] = controller;
    const isCanceled = () => controller.signal.aborted;
    const finish = (state: ApiModelLatencyState) => {
      setApiModelLatencies((current) => ({ ...current, [api.id]: state }));
      setLatencyTestingApiIds((current) => current.filter((id) => id !== api.id));
      if (latencyAbortControllersRef.current[api.id] === controller) {
        delete latencyAbortControllersRef.current[api.id];
      }
      return state;
    };
    setLatencyTestingApiIds((current) =>
      current.includes(api.id) ? current : [...current, api.id],
    );
    setApiModelLatencies((current) => ({
      ...current,
      [api.id]: {
        status: "testing",
        testedAt: null,
        successCount: 0,
        failCount: 0,
        fastestMs: null,
        testedModelCount: 0,
        totalModelCount: 1,
        testedAllModels: false,
        models: [
          {
            model: "模型列表探测中",
            ok: false,
            latencyMs: null,
            error: "优先读取该 API 的 /v1/models 候选",
          },
        ],
      },
    }));

    let probeCandidateModels: string[] = [];
    try {
      const probe = await modelRouterClient.runProbe(api.id, {
        signal: controller.signal,
      });
      if (isCanceled()) {
        throw new Error("测响已取消");
      }
      const candidates: string[] = [];
      appendUniqueModels(
        candidates,
        probe.candidates
          .filter((candidate) => !candidate.error)
          .map((candidate) => candidate.model),
      );
      probeCandidateModels = candidates;
      await queryClient.invalidateQueries({ queryKey: ["model-router", "probes"] });
    } catch (error) {
      if (isCanceled()) {
        const state: ApiModelLatencyState = {
          status: "failed",
          testedAt: Math.floor(Date.now() / 1000),
          successCount: 0,
          failCount: 1,
          fastestMs: null,
          testedModelCount: 0,
          totalModelCount: 0,
          testedAllModels: false,
          models: [
            {
              model: "测响已取消",
              ok: false,
              latencyMs: null,
              error: "用户取消",
            },
          ],
        };
        return finish(state);
      }
      void error;
    }

    const { models, source: modelSource } = buildLatencyTestModels(api, probeCandidateModels);

    if (models.length === 0) {
      const state: ApiModelLatencyState = {
        status: "failed",
        testedAt: Math.floor(Date.now() / 1000),
        successCount: 0,
        failCount: 1,
        fastestMs: null,
        testedModelCount: 0,
        totalModelCount: 0,
        testedAllModels: true,
        models: [
          {
            model: "未发现可测模型",
            ok: false,
            latencyMs: null,
            error:
              "该 API 未返回模型列表，且没有绑定模型；请在设置中添加测响兜底模型。",
          },
        ],
      };
      return finish(state);
    }

    const previousState = apiModelLatencies[api.id];
    const shouldTestAll =
      forceAllModels ||
      (previousState != null &&
        previousState.totalModelCount > previousState.testedModelCount &&
        !previousState.testedAllModels);
    const targetModels = shouldTestAll ? models : models.slice(0, 10);
    const testedAllModels = targetModels.length >= models.length;

    setApiModelLatencies((current) => ({
      ...current,
      [api.id]: {
        status: "testing",
        testedAt: null,
        successCount: 0,
        failCount: 0,
        fastestMs: null,
        testedModelCount: targetModels.length,
        totalModelCount: models.length,
        testedAllModels,
        models: targetModels.map((model) => ({
          model,
          ok: false,
          latencyMs: null,
          error: `短流式真实请求中，来源：${modelSource}`,
        })),
      },
    }));

    const results: ApiModelLatencyState["models"] = [];
    for (const model of targetModels) {
      if (isCanceled()) {
        results.push({
          model,
          ok: false,
          latencyMs: null,
          error: "已取消测响",
        });
        break;
      }
      try {
        const result = await modelRouterClient.quickCheck(
          {
            aggregateApiId: api.id,
            model,
          },
          { signal: controller.signal },
        );
        if (isCanceled()) {
          results.push({
            model,
            ok: false,
            latencyMs: null,
            error: "已取消测响",
          });
          break;
        }
        results.push({
          model,
          ok: result.ok,
          latencyMs: result.ok ? result.latencyMs : null,
          error: result.ok ? null : result.error || String(result.statusCode || "失败"),
        });
      } catch (error) {
        results.push({
          model,
          ok: false,
          latencyMs: null,
          error: error instanceof Error ? error.message : String(error),
        });
      }
    }

    const successModels = results.filter((item) => item.ok);
    const state: ApiModelLatencyState = {
      status: isCanceled() ? "failed" : successModels.length > 0 ? "success" : "failed",
      testedAt: Math.floor(Date.now() / 1000),
      successCount: successModels.length,
      failCount: results.length - successModels.length,
      fastestMs:
        successModels.length > 0
          ? Math.min(...successModels.map((item) => item.latencyMs ?? Number.MAX_SAFE_INTEGER))
          : null,
      testedModelCount: results.length,
      totalModelCount: models.length,
      testedAllModels,
      models: results,
    };
    return finish(state);
  };

  const renderLatencyStatus = (api: AggregateApi) => {
    const state = apiModelLatencies[api.id];
    const isTesting = state?.status === "testing" || latencyTestingApiIds.includes(api.id);
    const badge = (
      <Badge
        className={cn(
          "w-fit gap-1 border",
          isTesting && "border-sky-500/60 bg-sky-500/15 text-sky-500",
          state?.status === "success" &&
            "border-emerald-500/60 bg-emerald-500/15 text-emerald-500",
          state?.status === "failed" && "border-red-500/60 bg-red-500/15 text-red-500",
          !state && !isTesting && "border-border bg-muted text-muted-foreground",
        )}
      >
        <Timer className={cn("h-3 w-3", isTesting && "animate-pulse")} />
        {isTesting
          ? "测试中"
          : state?.status === "success"
            ? `成功 ${state.successCount}/${state.testedModelCount} · ${state.fastestMs} ms`
            : state?.status === "failed"
              ? `失败 ${state.failCount}/${state.testedModelCount || state.totalModelCount || 1}`
              : "未测响"}
      </Badge>
    );

    if (!state?.models.length) return badge;
    return (
      <Tooltip>
        <TooltipTrigger render={<div />} className="inline-flex cursor-help">
          {badge}
        </TooltipTrigger>
        <TooltipContent className="max-w-md whitespace-pre-wrap break-words">
          <div className="grid gap-1 text-xs">
            <div className="font-medium text-foreground">
              已测 {state.testedModelCount}/{state.totalModelCount} 个模型
              {state.testedAllModels ? "" : "，再次点击该行测响将测试全部模型"}
            </div>
            {state.models.map((item) => (
              <div key={item.model} className={item.ok ? "text-emerald-500" : "text-red-500"}>
                {item.model}: {item.ok ? `${item.latencyMs} ms` : item.error || "失败"}
              </div>
            ))}
          </div>
        </TooltipContent>
      </Tooltip>
    );
  };

  const testMutation = useMutation({
    mutationFn: (apiId: string) =>
      accountClient.testAggregateApiConnection(apiId),
    onMutate: async (apiId) => {
      setTestingApiId(apiId);
    },
    onSuccess: async (result) => {
      if (result.ok) {
        toast.success(t("已连通"));
        return;
      }
      toast.error(
        t("连通性测试失败: {reason}", {
          reason: result.message || result.statusCode || t("未返回具体错误信息"),
        }),
      );
    },
    onSettled: async (_result, _error, apiId) => {
      await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
      setTestingApiId((current) => (current === apiId ? null : current));
    },
    onError: (error: unknown) => {
      toast.error(`${t("测试")} ${t("失败")}: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const testAllMutation = useMutation({
    mutationFn: async (apiIds: string[]) => {
      const results = await Promise.allSettled(
        apiIds.map((id) => accountClient.testAggregateApiConnection(id))
      );
      return results;
    },
    onMutate: async () => {
      setTestingAll(true);
    },
    onSuccess: async (results) => {
      const successCount = results.filter(
        (r) => r.status === "fulfilled" && r.value.ok
      ).length;
      const failCount = results.length - successCount;

      if (failCount === 0) {
        toast.success(t("全部测试完成，{count} 个连通", { count: successCount }));
      } else {
        toast.warning(
          t("测试完成：{success} 个连通，{fail} 个失败", {
            success: successCount,
            fail: failCount,
          })
        );
      }
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
      setTestingAll(false);
    },
    onError: (error: unknown) => {
      toast.error(`${t("批量测试失败")}: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const testGroupMutation = useMutation({
    mutationFn: async (apis: AggregateApi[]) => {
      setTestingAll(true);
      return runWithConcurrency(apis, 5, async (api) => {
        const connection = await accountClient
          .testAggregateApiConnection(api.id)
          .then((result) => ({
            ok: Boolean(result.ok),
            error: result.ok
              ? null
              : result.message || String(result.statusCode || "连通性测试失败"),
          }))
          .catch((error: unknown) => ({
            ok: false,
            error: error instanceof Error ? error.message : String(error),
          }));
        let latency: ApiModelLatencyState;
        try {
          latency = await runApiModelLatencyTest(api);
        } catch (error) {
          latency = {
            status: "failed",
            testedAt: Math.floor(Date.now() / 1000),
            successCount: 0,
            failCount: 1,
            fastestMs: null,
            testedModelCount: 0,
            totalModelCount: 0,
            testedAllModels: true,
            models: [
              {
                model: "响应测响失败",
                ok: false,
                latencyMs: null,
                error: error instanceof Error ? error.message : String(error),
              },
            ],
          };
          setApiModelLatencies((current) => ({ ...current, [api.id]: latency }));
          setLatencyTestingApiIds((current) => current.filter((id) => id !== api.id));
        }
        return { api, connection, latency };
      });
    },
    onSuccess: async (results) => {
      const connectionSuccess = results.filter((item) => item.connection.ok).length;
      const latencySuccess = results.filter((item) => item.latency.status === "success").length;
      await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
      await queryClient.invalidateQueries({ queryKey: ["aggregate-api-model-usage"] });
      toast.success(
        `组测完成：连通 ${connectionSuccess}/${results.length}，响应 ${latencySuccess}/${results.length}`,
      );
    },
    onSettled: () => {
      setTestingAll(false);
    },
    onError: (error: unknown) => {
      setTestingAll(false);
      toast.error(`组测失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const testAllLatencyMutation = useMutation({
    mutationFn: async (apis: AggregateApi[]) => {
      latencyBatchCanceledRef.current = false;
      setTestingLatencyAll(true);
      return runWithConcurrency(apis, 5, async (api) => {
        if (latencyBatchCanceledRef.current) {
          throw new Error("测响已取消");
        }
        try {
          return { api, state: await runApiModelLatencyTest(api) };
        } catch (error) {
          const state: ApiModelLatencyState = {
            status: "failed",
            testedAt: Math.floor(Date.now() / 1000),
            successCount: 0,
            failCount: 1,
            fastestMs: null,
            testedModelCount: 0,
            totalModelCount: 0,
            testedAllModels: true,
            models: [
              {
                model: "测响失败",
                ok: false,
                latencyMs: null,
                error: error instanceof Error ? error.message : String(error),
              },
            ],
          };
          setApiModelLatencies((current) => ({ ...current, [api.id]: state }));
          setLatencyTestingApiIds((current) => current.filter((id) => id !== api.id));
          return { api, state };
        }
      }, () => latencyBatchCanceledRef.current);
    },
    onSuccess: (results) => {
      if (latencyBatchCanceledRef.current) {
        toast.info(`测响已取消：已完成 ${results.length}/${latencyTargetApis.length} 个 API`);
        return;
      }
      const successCount = results.filter((item) => item.state.status === "success").length;
      const failCount = results.length - successCount;
      toast.success(`响应测响完成：${successCount}/${results.length} 个 API 有响应，${failCount} 个失败`);
    },
    onSettled: () => {
      setTestingLatencyAll(false);
    },
    onError: (error: unknown) => {
      setTestingLatencyAll(false);
      toast.error(`响应测响失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const applyResponsiveStatusMutation = useMutation({
    mutationFn: async (apis: AggregateApi[]) => {
      return runWithConcurrency(apis, 5, async (api) => {
        try {
          const state = apiModelLatencies[api.id] ?? (await runApiModelLatencyTest(api));
          const enabled = state.status === "success";
          await accountClient.updateAggregateApi(api.id, {
            supplierName: api.supplierName || api.url,
            status: enabled ? "active" : "disabled",
          });
          return { api, ok: true, error: null as string | null };
        } catch (error) {
          return {
            api,
            ok: false,
            error: error instanceof Error ? error.message : String(error),
          };
        }
      });
    },
    onSuccess: async (results) => {
      const failures = results
        .filter((item) => !item.ok)
        .map((item) => `${item.api.supplierName || item.api.url || item.api.id}: ${item.error}`);
      await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
      if (failures.length > 0) {
        toast.warning(
          `按响应启停完成，${failures.length} 个失败：${failures.slice(0, 3).join("；")}`,
        );
        return;
      }
      toast.success(`已按响应结果更新 ${results.length} 个 API 的启用状态`);
    },
    onError: (error: unknown) => {
      toast.error(`按响应启停失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (apiId: string) => accountClient.deleteAggregateApi(apiId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
      await queryClient.invalidateQueries({ queryKey: ["apikeys"] });
      await queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] });
      await queryClient.invalidateQueries({ queryKey: ["model-router", "bindings"] });
      toast.success(`${t("聚合API")} ${t("删除")}`);
    },
    onError: (error: unknown) => {
      toast.error(`${t("删除")} ${t("失败")}: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const prioritizeMutation = useMutation({
    mutationFn: async (api: AggregateApi) => {
      const currentMinSort = aggregateApis.reduce(
        (min, item) => Math.min(min, Number(item.sort) || 0),
        Number(api.sort) || 0,
      );
      const nextSort =
        (Number(api.sort) || 0) <= currentMinSort ? currentMinSort : currentMinSort - 5;

      if ((Number(api.sort) || 0) === nextSort) {
        return false;
      }

      await accountClient.updateAggregateApi(api.id, {
        providerType: api.providerType,
        supplierName: api.supplierName || "",
        sort: nextSort,
        url: api.url,
        key: null,
      });
      return true;
    },
    onSuccess: async (changed) => {
      if (!changed) {
        toast.info(t("设为优先"));
        return;
      }
      await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
      toast.success(t("设为优先"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("设为优先")} ${t("失败")}: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const toggleStatusMutation = useMutation({
    mutationFn: async ({
      api,
      enabled,
    }: {
      api: AggregateApi;
      enabled: boolean;
    }) => {
      await accountClient.updateAggregateApi(api.id, {
        supplierName: api.supplierName || api.url,
        status: enabled ? "active" : "disabled",
      });
      return enabled;
    },
    onMutate: async ({ api, enabled }) => {
      await queryClient.cancelQueries({ queryKey: ["aggregate-apis"] });
      const previousAggregateApis =
        queryClient.getQueryData<AggregateApi[]>(["aggregate-apis"]) || [];
      setStatusOverrides((current) => ({
        ...current,
        [api.id]: enabled,
      }));
      queryClient.setQueryData<AggregateApi[]>(["aggregate-apis"], (current) =>
        (current || []).map((item) =>
          item.id === api.id
            ? {
                ...item,
                status: enabled ? "active" : "disabled",
              }
            : item,
        ),
      );
      setTogglingApiId(api.id);
      return {
        previousAggregateApis,
      };
    },
    onSuccess: async (_result, variables) => {
      setStatusOverrides((current) => ({
        ...current,
        [variables.api.id]: variables.enabled,
      }));
      queryClient.setQueryData<AggregateApi[]>(["aggregate-apis"], (current) =>
        (current || []).map((item) =>
          item.id === variables.api.id
            ? {
                ...item,
                status: variables.enabled ? "active" : "disabled",
              }
            : item,
        ),
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] }),
        queryClient.invalidateQueries({ queryKey: ["apikeys"] }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
      ]);
      toast.success(t("状态已更新"));
    },
    onError: (error: unknown, _variables, context) => {
      if (context?.previousAggregateApis) {
        queryClient.setQueryData(
          ["aggregate-apis"],
          context.previousAggregateApis,
        );
      }
      setStatusOverrides((current) => {
        const next = { ...current };
        if (_variables?.api?.id) {
          delete next[_variables.api.id];
        }
        return next;
      });
      toast.error(
        `${t("更新状态失败")}: ${error instanceof Error ? error.message : String(error)}`,
      );
    },
    onSettled: async (_result, _error, variables) => {
      setTogglingApiId((current) =>
        current === variables.api.id ? null : current,
      );
    },
  });

  const saveRouteBindingsMutation = useMutation({
    mutationFn: async ({
      apis,
      modelDraft,
      candidates,
    }: {
      apis: AggregateApi[];
      modelDraft: string;
      candidates: ProbeCandidateSummary[];
    }) => {
      const manualModels = modelDraft
        .split(/[\n,，;；\s]+/)
        .map((item) => item.trim())
        .filter(Boolean);
      const candidateModels = candidates.map((candidate) => candidate.model);
      const models = Array.from(new Set([...manualModels, ...candidateModels]));
      if (models.length === 0) {
        throw new Error("请先输入模型名，或先探测后选择候选模型");
      }
      const candidateByModel = new Map(candidates.map((candidate) => [candidate.model, candidate]));
      for (const api of apis) {
        for (const model of models) {
          const candidate = candidateByModel.get(model);
          await modelRouterClient.saveBinding({
            model,
            aggregateApiId: api.id,
            enabled: true,
            priority: api.sort,
            weight: 1,
            routeStrategy: "ordered",
            manualPreferred: false,
            supportsResponses: candidate?.supportsResponses ?? true,
            supportsChatCompletions: candidate?.supportsChatCompletions ?? false,
            requiresAdapter: candidate?.requiresAdapter ?? false,
          });
        }
      }
      return models;
    },
    onSuccess: async (models) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["model-router", "bindings"] }),
        queryClient.invalidateQueries({ queryKey: ["model-router", "probes"] }),
      ]);
      setBindingDialog(null);
      toast.success(`已绑定 ${models.length} 个模型`);
    },
    onError: (error: unknown) => {
      toast.error(`绑定模型失败: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const setFilteredSelection = (checked: boolean) => {
    setSelectedApiIds((current) => {
      const currentSet = new Set(current);
      if (checked) {
        filteredApiIds.forEach((id) => currentSet.add(id));
      } else {
        filteredApiIds.forEach((id) => currentSet.delete(id));
      }
      return Array.from(currentSet);
    });
  };

  const toggleApiSelection = (apiId: string, checked: boolean) => {
    setSelectedApiIds((current) => {
      const currentSet = new Set(current);
      if (checked) {
        currentSet.add(apiId);
      } else {
        currentSet.delete(apiId);
      }
      return Array.from(currentSet);
    });
  };

  const updateApisStatus = async (apis: AggregateApi[], enabled: boolean) => {
    if (apis.length === 0) {
      toast.info("请先选择聚合 API");
      return;
    }
    setBulkOperation(enabled ? "enable" : "disable");
    const results = await runWithConcurrency(apis, 5, async (api) => {
      try {
        await accountClient.updateAggregateApi(api.id, {
          supplierName: api.supplierName || api.url,
          status: enabled ? "active" : "disabled",
        });
        setStatusOverrides((current) => ({ ...current, [api.id]: enabled }));
        return { api, ok: true, error: null as string | null };
      } catch (error) {
        return {
          api,
          ok: false,
          error: error instanceof Error ? error.message : String(error),
        };
      }
    });
    const failedIds = results.filter((item) => !item.ok).map((item) => item.api.id);
    const failures = results
      .filter((item) => !item.ok)
      .map((item) => `${item.api.supplierName || item.api.url || item.api.id}: ${item.error}`);
    await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
    setBulkOperation(null);
    if (failures.length > 0) {
      setSelectedApiIds((current) => current.filter((id) => failedIds.includes(id)));
      toast.warning(
        `${enabled ? "启用" : "禁用"}完成，${failures.length} 个失败：${failures
          .slice(0, 3)
          .join("；")}`,
      );
      return;
    }
    setSelectedApiIds([]);
    toast.success(`已${enabled ? "启用" : "禁用"} ${apis.length} 个聚合 API`);
  };

  const updateSelectedApisStatus = async (enabled: boolean) => {
    await updateApisStatus(selectedApis, enabled);
  };

  const deleteSelectedApis = async () => {
    if (selectedApis.length === 0) {
      setConfirmBulkDeleteOpen(false);
      return;
    }
    setBulkOperation("delete");
    const results = await runWithConcurrency(selectedApis, 5, async (api) => {
      try {
        await accountClient.deleteAggregateApi(api.id);
        return { api, ok: true, error: null as string | null };
      } catch (error) {
        return {
          api,
          ok: false,
          error: error instanceof Error ? error.message : String(error),
        };
      }
    });
    const failedIds = results.filter((item) => !item.ok).map((item) => item.api.id);
    const failures = results
      .filter((item) => !item.ok)
      .map((item) => `${item.api.supplierName || item.api.url || item.api.id}: ${item.error}`);
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] }),
      queryClient.invalidateQueries({ queryKey: ["apikeys"] }),
      queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
    ]);
    if (failures.length === 0) {
      setSelectedApiIds([]);
      toast.success(`已删除 ${selectedApis.length} 个聚合 API`);
    } else {
      setSelectedApiIds((current) => current.filter((id) => failedIds.includes(id)));
      toast.warning(`删除完成，${failures.length} 个失败：${failures.slice(0, 3).join("；")}`);
    }
    setBulkOperation(null);
    setConfirmBulkDeleteOpen(false);
  };

  /**
   * 函数 `openCreateModal`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * 无
   *
   * # 返回
   * 返回函数执行结果
   */
  const openCreateModal = () => {
    setEditingId(null);
    setTemplateApiId(null);
    setModalOpen(true);
  };

  const openCreateFromGroup = (api: AggregateApi) => {
    setEditingId(null);
    setTemplateApiId(api.id);
    setModalOpen(true);
  };

  /**
   * 函数 `openEditModal`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - apiId: 参数 apiId
   *
   * # 返回
   * 返回函数执行结果
   */
  const openEditModal = (apiId: string) => {
    setEditingId(apiId);
    setTemplateApiId(null);
    setModalOpen(true);
  };

  const openBindingDialog = (api: AggregateApi) => {
    setBindingDialog({
      apiId: api.id,
      apiIds: [api.id],
      modelDraft: "",
      selectedCandidateIds: [],
      probeCandidates: [],
      probing: false,
    });
  };

  const openGroupBindingDialog = (apis: AggregateApi[]) => {
    const target = apis.find((api) => String(api.status || "").trim().toLowerCase() !== "disabled") ?? apis[0];
    if (!target) return;
    setBindingDialog({
      apiId: target.id,
      apiIds: apis.map((api) => api.id),
      modelDraft: "",
      selectedCandidateIds: [],
      probeCandidates: [],
      probing: false,
    });
  };

  const runBindingProbe = async (api: AggregateApi) => {
    setBindingDialog((current) =>
      current?.apiId === api.id ? { ...current, probing: true } : current,
    );
    try {
      const probe = await modelRouterClient.runProbe(api.id);
      const selectable = probe.candidates.filter(
        (candidate) =>
          !candidate.error &&
          (candidate.supportsResponses || candidate.supportsChatCompletions),
      );
      setBindingDialog((current) =>
        current?.apiId === api.id
          ? {
              ...current,
              probing: false,
              probeCandidates: selectable,
              selectedCandidateIds: selectable.map((candidate) => candidate.id),
            }
          : current,
      );
      await queryClient.invalidateQueries({ queryKey: ["model-router", "probes"] });
      toast.success(`探测到 ${selectable.length} 个可绑定模型`);
    } catch (error) {
      setBindingDialog((current) =>
        current?.apiId === api.id ? { ...current, probing: false } : current,
      );
      toast.error(`探测模型失败: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const bindingImpactModelsForApi = (apiId: string) =>
    (routeBindingsByAggregateApiId.get(apiId) ?? []).map((binding) => binding.model);

  const toggleGroupExpanded = (groupKey: string) => {
    setExpandedGroupKeys((current) =>
      current.includes(groupKey)
        ? current.filter((key) => key !== groupKey)
        : [...current, groupKey],
    );
  };

  const updateApiSort = async (api: AggregateApi, sort: number) => {
    await accountClient.updateAggregateApi(api.id, {
      providerType: api.providerType,
      supplierName: api.supplierName || api.url,
      sort,
      url: api.url,
      authType: api.authType,
      authParams: api.authParams,
      action: api.action,
      key: null,
    });
    await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
  };

  /**
   * 函数 `ensureSecretLoaded`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - apiId: 参数 apiId
   *
   * # 返回
   * 返回函数执行结果
   */
  const ensureSecretLoaded = async (apiId: string) => {
    if (revealedSecrets[apiId]) {
      return revealedSecrets[apiId];
    }
    setLoadingSecretId(apiId);
    try {
      const secretResult = await accountClient.readAggregateApiSecret(apiId);
      const authType = String(secretResult.authType || "").trim().toLowerCase();
      if (authType === "userpass") {
        if (!secretResult.username || !secretResult.password) {
          throw new Error(t("后端未返回账号密码明文"));
        }
      } else if (!secretResult.key) {
        throw new Error(t("后端未返回密钥明文"));
      }
      setRevealedSecrets((current) => ({ ...current, [apiId]: secretResult }));
      return secretResult;
    } finally {
      setLoadingSecretId(null);
    }
  };

  /**
   * 函数 `toggleSecret`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - apiId: 参数 apiId
   *
   * # 返回
   * 返回函数执行结果
   */
  const toggleSecret = async (apiId: string) => {
    if (revealedSecrets[apiId]) {
      setRevealedSecrets((current) => {
        const next = { ...current };
        delete next[apiId];
        return next;
      });
      return;
    }
    try {
      await ensureSecretLoaded(apiId);
    } catch (error: unknown) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  /**
   * 函数 `copySecret`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - apiId: 参数 apiId
   *
   * # 返回
   * 返回函数执行结果
   */
  const copySecret = async (
    apiId: string,
    target: "key" | "username" | "password"
  ) => {
    try {
      const secret = await ensureSecretLoaded(apiId);
      const authType = String(secret.authType || "").trim().toLowerCase();
      const value =
        target === "username"
          ? secret.username
          : target === "password"
            ? secret.password
            : secret.key;
      if (authType === "userpass") {
        if (!value) {
          throw new Error(t("账号密码字段为空"));
        }
      } else if (!value) {
        throw new Error(t("密钥为空"));
      }
      await copyTextToClipboard(value);
      toast.success(t("已复制到剪贴板"));
    } catch (error: unknown) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const secretPreview = (secret: AggregateApiSecretResult) => {
    const authType = String(secret.authType || "").trim().toLowerCase();
    if (authType === "userpass") {
      return `${secret.username || ""}:${secret.password || ""}`;
    }
    return secret.key || "";
  };

  const deleteTargetApi = deleteId ? aggregateApiById.get(deleteId) : null;
  const deleteTargetModels = deleteId ? bindingImpactModelsForApi(deleteId) : [];
  const selectedDeleteBindingCount = selectedApis.reduce(
    (sum, api) => sum + bindingImpactModelsForApi(api.id).length,
    0,
  );

  return (
    <div className="space-y-6 animate-in fade-in duration-500 [&_.glass-card]:border [&_.glass-card]:border-border [&_.glass-card]:shadow-lg">
      {!isServiceReady ? (
        <Card className="glass-card shadow-sm">
          <CardContent className="pt-6 text-sm text-muted-foreground">
            {t("服务未连接")}
          </CardContent>
        </Card>
      ) : null}

      <div>
        <div>
          <p className="mt-1 text-sm text-muted-foreground">
            {poolFilter === "wool"
              ? "管理短时效免费 API。羊毛池会优先尝试，失败、冷却或满载后自动回退主用 API。"
              : t("管理上游聚合地址与密钥，并测试连通性")}
          </p>
        </div>
      </div>

      <div className="space-y-4">
        <Card className="glass-card shadow-xl backdrop-blur-md">
          <CardContent className="px-4 ">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center gap-2">
                <div className="flex rounded-md border border-border bg-background/70 p-1">
                  <Button
                    type="button"
                    variant={poolFilter === "primary" ? "secondary" : "ghost"}
                    size="sm"
                    className="h-8 px-3"
                    onClick={() => setPoolFilter("primary")}
                  >
                    主用
                  </Button>
                  <Button
                    type="button"
                    variant={poolFilter === "wool" ? "secondary" : "ghost"}
                    size="sm"
                    className="h-8 gap-1.5 px-3"
                    onClick={() => setPoolFilter("wool")}
                  >
                    <Sparkles className="h-3.5 w-3.5" />
                    羊毛
                  </Button>
                </div>
                <span className="text-sm text-muted-foreground">{t("查询")}</span>
                <Select
                  value={providerFilter}
                  onValueChange={(value) => setProviderFilter(value || "all")}
                >
                  <SelectTrigger className="w-[160px]">
                    <SelectValue>
                      {(value) =>
                        t(
                          AGGREGATE_API_PROVIDER_FILTER_LABELS[
                            String(value || "")
                          ] || "全部类型",
                        )
                      }
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">{t("全部类型")}</SelectItem>
                    <SelectItem value="codex">Codex</SelectItem>
                    <SelectItem value="claude">Claude</SelectItem>
                  </SelectContent>
                </Select>
                <span className="text-xs text-muted-foreground">
                  当前: {tableWidthBucketLabel(tableWidthMode)}
                </span>
                <Select
                  value={selectedTableWidthProfileId || "__none__"}
                  onValueChange={(value) =>
                    setSelectedTableWidthProfileId(
                      !value || value === "__none__" ? "" : value,
                    )
                  }
                >
                  <SelectTrigger className="h-8 w-[180px] text-xs">
                    <SelectValue>
                      {(value) => {
                        const id = String(value || "");
                        return tableWidthProfiles.find((item) => item.id === id)?.name || "选择列宽方案";
                      }}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__none__">选择列宽方案</SelectItem>
                    {tableWidthProfiles.map((profile) => (
                      <SelectItem key={profile.id} value={profile.id}>
                        {profile.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Button
                  variant="default"
                  size="sm"
                  className="h-8 gap-1.5 px-3 text-xs"
                  onClick={saveTableColumnWidths}
                >
                  命名保存列宽
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-8 gap-1.5 px-3 text-xs"
                  onClick={applySavedTableColumnWidths}
                >
                  应用
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-8 gap-1.5 px-3 text-xs"
                  onClick={deleteSelectedTableWidthProfile}
                  disabled={!selectedTableWidthProfileId}
                >
                  删除
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-8 gap-1.5 px-3 text-xs"
                  onClick={resetTableColumnWidths}
                >
                  重置
                </Button>
              </div>
              <div className="flex items-center gap-3">
                <div className="text-xs text-muted-foreground">
                  {t("共")} {filteredAggregateApis.length} {t("条")}
                  {selectedApiIds.length > 0 ? ` · 已选 ${selectedApiIds.length}` : ""}
                </div>
                <Button
                  variant="outline"
                  className="h-10 gap-2"
                  onClick={() => {
                    const apiIds = filteredAggregateApis.map((api) => api.id);
                    if (apiIds.length === 0) {
                      toast.info(t("暂无可测试的聚合 API"));
                      return;
                    }
                    testAllMutation.mutate(apiIds);
                  }}
                  disabled={!isServiceReady || testingAll || filteredAggregateApis.length === 0}
                >
                  <RefreshCw className={testingAll ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
                  {t("测试全部")}
                </Button>
                <Button
                  variant="outline"
                  className="h-10 gap-2"
                  onClick={() => testAllLatencyMutation.mutate(latencyTargetApis)}
                  disabled={!isServiceReady || testingLatencyAll || latencyTargetApis.length === 0}
                >
                  <Timer className={testingLatencyAll ? "h-4 w-4 animate-pulse" : "h-4 w-4"} />
                  一键测响
                </Button>
                <Button
                  variant="outline"
                  className="h-10 gap-2 text-amber-600 hover:text-amber-700"
                  onClick={cancelLatencyTests}
                  disabled={!testingLatencyAll && latencyTestingApiIds.length === 0}
                  title="停止后续测响请求；已发出的单个请求会等当前调用返回"
                >
                  取消测响
                </Button>
                <Button
                  variant="outline"
                  className="h-10 gap-2"
                  onClick={() => void updateSelectedApisStatus(true)}
                  disabled={!isServiceReady || bulkOperation !== null || selectedApis.length === 0}
                >
                  启用所选
                </Button>
                <Button
                  variant="outline"
                  className="h-10 gap-2"
                  onClick={() => void updateSelectedApisStatus(false)}
                  disabled={!isServiceReady || bulkOperation !== null || selectedApis.length === 0}
                >
                  禁用所选
                </Button>
                <Button
                  variant="outline"
                  className="h-10 gap-2 text-red-500 hover:text-red-600"
                  onClick={() => setConfirmBulkDeleteOpen(true)}
                  disabled={!isServiceReady || bulkOperation !== null || selectedApis.length === 0}
                >
                  <Trash2 className="h-4 w-4" />
                  删除所选
                </Button>
                <Button
                  variant="outline"
                  className="h-10 gap-2"
                  onClick={() => applyResponsiveStatusMutation.mutate(latencyTargetApis)}
                  disabled={
                    !isServiceReady ||
                    applyResponsiveStatusMutation.isPending ||
                    latencyTargetApis.length === 0
                  }
                >
                  按响应启停
                </Button>
                <Button
                  className="h-10 gap-2 shadow-lg shadow-primary/20"
                  onClick={openCreateModal}
                  disabled={!isServiceReady}
                >
                  <Plus className="h-4 w-4" /> {t("新建聚合 API")}
                </Button>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card className="glass-card overflow-hidden py-0 shadow-xl backdrop-blur-md">
          <CardContent className="p-0">
            <div className="subtle-scrollbar overflow-x-auto overflow-y-hidden">
            <Table
              className="table-fixed [&_td]:border-b [&_td]:border-r [&_td]:border-border [&_th]:border-b [&_th]:border-r [&_th]:border-border"
              style={{ width: tablePixelWidth, minWidth: tablePixelWidth }}
            >
              <colgroup>
                <col style={{ width: tableColumnWidths.select }} />
                <col style={{ width: tableColumnWidths.provider }} />
                <col style={{ width: tableColumnWidths.secret }} />
                <col style={{ width: tableColumnWidths.sort }} />
                <col style={{ width: tableColumnWidths.test }} />
                <col style={{ width: tableColumnWidths.latency }} />
                <col style={{ width: tableColumnWidths.status }} />
                <col style={{ width: tableColumnWidths.action }} />
              </colgroup>
              <TableHeader>
                <TableRow>
                  <TableHead className="border-r border-border bg-muted/50 text-center">
                    <Checkbox
                      checked={allFilteredSelected}
                      aria-label="选择当前筛选结果全部 API"
                      className={cn(someFilteredSelected && !allFilteredSelected && "bg-primary/30")}
                      onCheckedChange={(checked) => setFilteredSelection(Boolean(checked))}
                    />
                  </TableHead>
                  <TableHead className="relative border-r border-border bg-muted/50 px-3 shadow-[inset_-1px_0_0_hsl(var(--border))]">
                    {t("供应商 / URL")}
                    <HeaderResizeHandle
                      column="provider"
                      onResizeStart={resizeTableColumn}
                    />
                  </TableHead>
                  <TableHead className="relative border-r border-border bg-muted/50 px-3 shadow-[inset_-1px_0_0_hsl(var(--border))]">
                    {t("密钥")}
                    <HeaderResizeHandle
                      column="secret"
                      onResizeStart={resizeTableColumn}
                    />
                  </TableHead>
                  <TableHead className="relative border-r border-border bg-muted/50 px-3 text-center shadow-[inset_-1px_0_0_hsl(var(--border))]">
                    {t("顺序")}
                    <HeaderResizeHandle
                      column="sort"
                      onResizeStart={resizeTableColumn}
                    />
                  </TableHead>
                  <TableHead className="relative border-r border-border bg-muted/50 px-3 shadow-[inset_-1px_0_0_hsl(var(--border))]">
                    {t("测试连通性")}
                    <HeaderResizeHandle column="test" onResizeStart={resizeTableColumn} />
                  </TableHead>
                  <TableHead className="relative border-r border-border bg-muted/50 px-3 shadow-[inset_-1px_0_0_hsl(var(--border))]">
                    响应速度
                    <HeaderResizeHandle
                      column="latency"
                      onResizeStart={resizeTableColumn}
                    />
                  </TableHead>
                  <TableHead className="relative border-r border-border bg-muted/50 px-3 text-center shadow-[inset_-1px_0_0_hsl(var(--border))]">
                    状态
                    <HeaderResizeHandle
                      column="status"
                      onResizeStart={resizeTableColumn}
                    />
                  </TableHead>
                  <TableHead className="relative border-r border-border bg-muted/50 px-3 text-center shadow-[inset_-1px_0_0_hsl(var(--border))]">
                    操作
                    <HeaderResizeHandle
                      column="action"
                      onResizeStart={resizeTableColumn}
                    />
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLoading ? (
                  Array.from({ length: 3 }).map((_, index) => (
                    <TableRow key={index}>
                      <TableCell>
                        <Skeleton className="mx-auto h-4 w-4 rounded" />
                      </TableCell>
                      <TableCell>
                        <Skeleton className="h-4 w-24" />
                      </TableCell>
                      <TableCell>
                        <Skeleton className="h-4 w-28" />
                      </TableCell>
                      <TableCell>
                        <Skeleton className="mx-auto h-4 w-12" />
                      </TableCell>
                      <TableCell>
                        <Skeleton className="h-6 w-20 rounded-full" />
                      </TableCell>
                      <TableCell>
                        <Skeleton className="h-6 w-20 rounded-full" />
                      </TableCell>
                      <TableCell>
                        <Skeleton className="h-6 w-16 rounded-full" />
                      </TableCell>
                      <TableCell className="text-center">
                        <Skeleton className="mx-auto h-8 w-20" />
                      </TableCell>
                    </TableRow>
                  ))
                ) : filteredAggregateApis.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={8} className="h-48 text-center">
                      <div className="flex flex-col items-center justify-center gap-2 text-muted-foreground">
                        <ShieldCheck className="h-8 w-8 opacity-20" />
                        <p>
                          {providerFilter === "all"
                            ? poolFilter === "wool"
                              ? "暂无羊毛 API，点击右上角新建"
                              : t("暂无聚合 API，点击右上角新建")
                            : t("暂无 {provider} 聚合 API", {
                                provider:
                                  AGGREGATE_API_PROVIDER_LABELS[
                                    providerFilter
                                  ] || providerFilter,
                              })}
                        </p>
                      </div>
                    </TableCell>
                  </TableRow>
                ) : (
                  aggregateApiGroups.flatMap((group) => {
                    const selectedInGroup = group.items.filter((api) =>
                      selectedApiIds.includes(api.id),
                    ).length;
                    const groupExpanded = expandedGroupKeys.includes(group.key);
                    const groupBoundModels = Array.from(
                      new Set(
                        group.items.flatMap((api) => bindingImpactModelsForApi(api.id)),
                      ),
                    );
                    const allGroupSelected =
                      group.items.length > 0 && selectedInGroup === group.items.length;
                    const groupRow = (
                      <TableRow
                        key={`group-${group.key}`}
                        className={cn(
                          "cursor-pointer border-b border-border hover:bg-muted/95",
                          groupRowBackground(group.items.length > 1, groupExpanded),
                        )}
                        onClick={() => toggleGroupExpanded(group.key)}
                      >
                        <TableCell
                          className="text-center"
                          onClick={(event) => event.stopPropagation()}
                        >
                          <Checkbox
                            checked={allGroupSelected}
                            aria-label={`选择 ${group.url} 全部 key`}
                            className={cn(
                              selectedInGroup > 0 && !allGroupSelected && "bg-primary/30",
                            )}
                            onCheckedChange={(checked) => {
                              setSelectedApiIds((current) => {
                                const next = new Set(current);
                                group.items.forEach((api) => {
                                  if (checked) next.add(api.id);
                                  else next.delete(api.id);
                                });
                                return Array.from(next);
                              });
                            }}
                          />
                        </TableCell>
                        <TableCell className="bg-transparent">
                          <div
                            role="button"
                            tabIndex={0}
                            className="flex w-full min-w-0 items-start gap-2 rounded-xl px-2 py-1.5 text-left outline-none transition-colors hover:bg-background/40 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
                            onClick={(event) => {
                              event.stopPropagation();
                              toggleGroupExpanded(group.key);
                            }}
                            onKeyDown={(event) => {
                              if (event.key === "Enter" || event.key === " ") {
                                event.preventDefault();
                                event.stopPropagation();
                                toggleGroupExpanded(group.key);
                              }
                            }}
                          >
                            {groupExpanded ? (
                              <ChevronDown className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
                            ) : (
                              <ChevronRight className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
                            )}
                            <div className="min-w-0 flex-1">
                              <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
                                <div className="min-w-[180px] flex-1">
                                  <div className="text-[10px] font-medium uppercase tracking-[0.18em] text-muted-foreground">
                                    Base URL
                                  </div>
                                  <div className="truncate text-sm font-black text-foreground">
                                    {group.url}
                                  </div>
                                </div>
                                <Tooltip>
                                  <TooltipTrigger
                                    render={<div />}
                                    className="min-w-0 cursor-help"
                                    onClick={(event) => event.stopPropagation()}
                                  >
                                    {group.modelUsages.length > 0 ? (
                                      <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                                        {group.modelUsages.slice(0, 2).map((usage) => (
                                          <Badge
                                            key={`${usage.aggregateApiUrl}-${usage.model}`}
                                            variant="secondary"
                                            className="max-w-[128px] truncate px-1.5 text-[10px] font-normal"
                                          >
                                            {usage.model}: {formatCompactNumber(usage.totalTokens)}
                                          </Badge>
                                        ))}
                                        {group.modelUsages.length > 2 ? (
                                          <span className="text-[10px] text-muted-foreground">
                                            +{group.modelUsages.length - 2}
                                          </span>
                                        ) : null}
                                        <span className="text-[11px] text-muted-foreground">
                                          {formatCompactNumber(group.usageTotalTokens)} tokens · {formatUsd(group.usageTotalCostUsd)} · {group.usageRequestCount} 次
                                        </span>
                                      </div>
                                    ) : (
                                      <span className="text-[11px] text-muted-foreground">
                                        暂无调用用量
                                      </span>
                                    )}
                                  </TooltipTrigger>
                                  <TooltipContent className="max-w-md whitespace-pre-wrap break-words">
                                    {group.modelUsages.length > 0 ? (
                                      <div className="grid gap-1 text-xs">
                                        <div className="font-medium text-foreground">
                                          {group.url}
                                        </div>
                                        {group.modelUsages.map((usage) => (
                                          <div key={`${usage.aggregateApiUrl}-${usage.model}`}>
                                            {usage.model || "unknown"}: {usage.requestCount} 次，{usage.totalTokens} tokens，估算 {formatUsd(usage.estimatedCostUsd)}
                                          </div>
                                        ))}
                                        <div className="border-t border-border pt-1 text-muted-foreground">
                                          合计: {group.usageRequestCount} 次，{group.usageTotalTokens} tokens，估算 {formatUsd(group.usageTotalCostUsd)}
                                        </div>
                                      </div>
                                    ) : (
                                      "暂无该 Base URL 的 token/费用记录"
                                    )}
                                  </TooltipContent>
                                </Tooltip>
                              </div>
                              <div className="mt-1 flex flex-wrap items-center gap-2">
                                <span className="font-medium">{group.items.length} 个 key</span>
                                {poolFilter === "wool" ? (
                                  <Badge className="border-amber-500/30 bg-amber-500/15 text-amber-600">
                                    羊毛池
                                  </Badge>
                                ) : null}
                                <span className="text-xs text-muted-foreground">
                                  最小顺序 {group.minSort}
                                </span>
                                <Badge variant="secondary">
                                  已绑定 {groupBoundModels.length} 模型
                                </Badge>
                              </div>
                            </div>
                          </div>
                        </TableCell>
                        <TableCell className="bg-transparent" />
                        <TableCell className="bg-transparent text-center font-mono text-xs text-muted-foreground">
                          {group.minSort}
                        </TableCell>
                        <TableCell className="bg-transparent">
                          <Badge variant="secondary">
                            连通 {group.items.filter((api) => api.lastTestStatus === "success").length}/
                            {group.items.length}
                          </Badge>
                        </TableCell>
                        <TableCell className="overflow-hidden bg-transparent">
                          <Badge
                            className={cn(
                              "max-w-full gap-1 truncate border",
                              group.fastestMs != null
                                ? "border-emerald-500/60 bg-emerald-500/15 text-emerald-500"
                                : "border-border bg-muted text-muted-foreground",
                            )}
                          >
                            <Timer className="h-3 w-3" />
                            {group.fastestMs != null
                              ? `${group.fastestMs} ms · 成功 ${group.successCount} 失败 ${group.failCount}`
                              : `成功 ${group.successCount} 失败 ${group.failCount}`}
                          </Badge>
                        </TableCell>
                        <TableCell
                          className="border-r border-border/90 bg-blue-50/35 px-2 dark:bg-slate-900/35"
                          onClick={(event) => event.stopPropagation()}
                        >
                          <div className="flex flex-col items-center justify-center gap-1">
                            <Badge variant="secondary">
                              启用 {group.items.filter((api) => String(api.status || "").trim().toLowerCase() !== "disabled").length}
                            </Badge>
                            <Button
                              variant="outline"
                              size="sm"
                              className="h-7 px-2 text-xs"
                              disabled={!isServiceReady || bulkOperation !== null}
                              onClick={() => {
                                setSelectedApiIds(group.items.map((api) => api.id));
                                void updateApisStatus(group.items, true);
                              }}
                              title="启用该 Base URL 下全部 API"
                            >
                              启用
                            </Button>
                          </div>
                        </TableCell>
                        <TableCell
                          className="bg-transparent px-2"
                          onClick={(event) => event.stopPropagation()}
                        >
                          <div className="flex flex-wrap items-center justify-center gap-1.5">
                            <Button
                              variant="outline"
                              size="sm"
                              className="h-7 gap-1 px-2 text-xs"
                              disabled={!isServiceReady}
                              onClick={() => openGroupBindingDialog(group.items)}
                              title="组内全部 key 绑定模型"
                            >
                              <Boxes className="h-3.5 w-3.5" />
                              <span>
                                组绑
                              </span>
                            </Button>
                            <Button
                              variant="outline"
                              size="sm"
                              className="h-7 gap-1 px-2 text-xs"
                              disabled={!isServiceReady || testGroupMutation.isPending}
                              onClick={() => testGroupMutation.mutate(group.items)}
                              title="组内全部 key 测试连通性和响应"
                            >
                              <RefreshCw className="h-3.5 w-3.5" />
                              <span>
                                组测
                              </span>
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7"
                              disabled={!isServiceReady}
                              onClick={() => openCreateFromGroup(group.items[0])}
                              title="新增 key"
                            >
                              <Plus className="h-4 w-4" />
                            </Button>
                          </div>
                        </TableCell>
                      </TableRow>
                    );
                    if (!groupExpanded) {
                      return [groupRow];
                    }
                      return [
                        groupRow,
                        ...group.items.map((api) => {
                    const revealed = revealedSecrets[api.id];
                    const serverEnabled =
                      String(api.status || "")
                        .trim()
                        .toLowerCase() !== "disabled";
                    const isEnabled = statusOverrides[api.id] ?? serverEnabled;
                    const boundModels = bindingImpactModelsForApi(api.id);
                    const createdTimeText = formatTsFromSeconds(
                      api.createdAt,
                      t("未知时间"),
                    );

                      return (
                        <TableRow
                          key={api.id}
                          className={cn("group border-b border-border/80 hover:bg-accent/25", keyRowBackground())}
                        >
                        <TableCell className="text-center">
                          <Checkbox
                            checked={selectedApiIds.includes(api.id)}
                            aria-label={`选择 ${api.supplierName || api.url || api.id}`}
                            onCheckedChange={(checked) =>
                              toggleApiSelection(api.id, Boolean(checked))
                            }
                          />
                        </TableCell>
                        <TableCell className="overflow-hidden bg-slate-50/80 dark:bg-slate-950/25">
                          <Tooltip>
                            <TooltipTrigger
                              render={<div />}
                              className="block cursor-help text-left"
                            >
                              <div className="grid gap-0.5 overflow-hidden">
                                <span className="block truncate text-xs font-medium text-foreground">
                                  {api.supplierName || "-"}
                                </span>
                                <span className="block truncate font-mono text-[11px] text-muted-foreground">
                                  {api.url}
                                </span>
                              </div>
                            </TooltipTrigger>
                            <TooltipContent className="max-w-sm whitespace-pre-wrap break-words">
                              <div className="grid gap-1">
                                <div className="text-[11px] font-medium">
                                  {api.supplierName || "-"}
                                </div>
                                <div className="break-all text-xs">
                                  {api.url}
                                </div>
                                <div className="text-[11px] opacity-80">
                                  {t("创建时间")}: {createdTimeText}
                                </div>
                                {boundModels.length > 0 ? (
                                  <div className="text-[11px] opacity-80">
                                    绑定模型: {boundModels.join(", ")}
                                  </div>
                                ) : null}
                              </div>
                            </TooltipContent>
                          </Tooltip>
                          <div className="mt-1 flex flex-wrap items-center gap-1">
                            {boundModels.length > 0 ? (
                              <Tooltip>
                                <TooltipTrigger render={<div />} className="cursor-help">
                                  <Badge variant="secondary" className="text-[10px]">
                                    绑定 {boundModels.length}
                                  </Badge>
                                </TooltipTrigger>
                                <TooltipContent className="max-w-sm whitespace-pre-wrap break-words">
                                  {boundModels.join("\n")}
                                </TooltipContent>
                              </Tooltip>
                            ) : (
                              <Badge variant="secondary" className="text-[10px]">
                                未绑定模型
                              </Badge>
                            )}
                            {!isEnabled && boundModels.length > 0 ? (
                              <Badge className="border-amber-500/30 bg-amber-500/15 text-amber-600">
                                绑定已停用
                              </Badge>
                            ) : null}
                            {api.fast ? (
                              <Badge className="border-sky-500/30 bg-sky-500/15 text-sky-600">
                                fast
                              </Badge>
                            ) : null}
                            {api.compatibilityMode ? (
                              <Badge className="border-violet-500/30 bg-violet-500/15 text-violet-600">
                                抗断流
                              </Badge>
                            ) : null}
                          </div>
                        </TableCell>
                        <TableCell className="overflow-hidden align-top bg-slate-50/80 dark:bg-slate-950/25">
                          <div className="flex min-w-0 flex-col gap-1.5 overflow-hidden py-1">
                            <Tooltip>
                              <TooltipTrigger
                                render={<div />}
                                className="block min-w-0 cursor-help"
                              >
                                <code className="block min-w-0 flex-1 truncate rounded-md border border-border bg-muted/60 px-2 py-1 font-mono text-[10px] leading-4 text-foreground">
                                  {revealed
                                    ? secretPreview(revealed)
                                    : loadingSecretId === api.id
                                      ? t("读取中...")
                                      : api.id}
                                </code>
                              </TooltipTrigger>
                              <TooltipContent className="max-w-sm whitespace-pre-wrap break-words">
                                {revealed ? secretPreview(revealed) : api.id}
                              </TooltipContent>
                            </Tooltip>
                            <div className="flex flex-wrap items-center gap-1">
                              <Button
                                variant="outline"
                                size="sm"
                                className="h-6 gap-1 px-2 text-[11px] text-muted-foreground hover:text-primary"
                                disabled={!isServiceReady}
                                onClick={() => void toggleSecret(api.id)}
                              >
                                {revealed ? (
                                  <EyeOff className="h-3.5 w-3.5" />
                                ) : (
                                  <Eye className="h-3.5 w-3.5" />
                                )}
                                {revealed ? "隐藏" : "查看"}
                              </Button>
                              {String(api.authType || "")
                                .trim()
                                .toLowerCase() === "userpass" ? (
                                <DropdownMenu>
                                  <DropdownMenuTrigger>
                                    <Button
                                      variant="outline"
                                      size="sm"
                                      className="h-6 gap-1 px-2 text-[11px] text-muted-foreground hover:text-primary"
                                      render={<span />}
                                      nativeButton={false}
                                      disabled={!isServiceReady}
                                    >
                                      <Copy className="h-3.5 w-3.5" />
                                      复制
                                    </Button>
                                  </DropdownMenuTrigger>
                                  <DropdownMenuContent align="end">
                                    <DropdownMenuItem
                                      onClick={() => void copySecret(api.id, "username")}
                                    >
                                      {t("复制用户名")}
                                    </DropdownMenuItem>
                                    <DropdownMenuItem
                                      onClick={() => void copySecret(api.id, "password")}
                                    >
                                      {t("复制密码")}
                                    </DropdownMenuItem>
                                  </DropdownMenuContent>
                                </DropdownMenu>
                              ) : (
                                <Button
                                  variant="outline"
                                  size="sm"
                                  className="h-6 gap-1 px-2 text-[11px] text-muted-foreground hover:text-primary"
                                  disabled={!isServiceReady}
                                  onClick={() => void copySecret(api.id, "key")}
                                >
                                  <Copy className="h-3.5 w-3.5" />
                                  复制
                                </Button>
                              )}
                            </div>
                          </div>
                        </TableCell>
                        <TableCell className="text-center">
                          <NumberStepper
                            value={api.sort}
                            ariaLabel={`调整 ${api.supplierName || api.id} 顺序`}
                            className="mx-auto w-16 [&_button]:w-4 [&_input]:px-0 [&_svg]:h-2.5 [&_svg]:w-2.5"
                            disabled={!isServiceReady}
                            onCommit={(value) => void updateApiSort(api, value)}
                          />
                        </TableCell>
                        <TableCell className="whitespace-nowrap align-middle bg-slate-50/80 dark:bg-slate-950/25">
                          <div className="flex flex-col items-start gap-1">
                            <div className="flex items-center gap-2">
                              {renderTestStatus(api)}
                              <Button
                                variant="outline"
                                size="sm"
                                className="h-7 gap-1.5 px-2 text-xs"
                                disabled={
                                  !isServiceReady || testingApiId === api.id
                                }
                                onClick={() => testMutation.mutate(api.id)}
                              >
                                <RefreshCw
                                  className={
                                    testingApiId === api.id
                                      ? "h-3.5 w-3.5 animate-spin"
                                      : "h-3.5 w-3.5"
                                  }
                                />
                                {t("测试")}
                              </Button>
                            </div>
                          </div>
                          {api.lastTestAt ? (
                            <p className="mt-1 text-[10px] text-muted-foreground">
                              {formatTsFromSeconds(api.lastTestAt, t("未知时间"))}
                            </p>
                          ) : null}
                          {api.lastTestStatus === "failed" && api.lastTestError ? (
                            <Tooltip>
                              <TooltipTrigger
                                render={<div />}
                                className="mt-1 block max-w-full cursor-help text-left"
                              >
                                <p className="max-w-[220px] truncate text-[10px] text-red-500/90">
                                  {api.lastTestError}
                                </p>
                              </TooltipTrigger>
                              <TooltipContent className="max-w-sm whitespace-pre-wrap break-words">
                                {api.lastTestError}
                              </TooltipContent>
                            </Tooltip>
                          ) : null}
                        </TableCell>
                        <TableCell className="whitespace-nowrap align-middle">
                          <div className="flex flex-col items-start gap-1">
                            <div className="flex items-center gap-2">
                              {renderLatencyStatus(api)}
                              <Button
                                variant="outline"
                                size="sm"
                                className="h-7 gap-1.5 px-2 text-xs"
                                disabled={
                                  !isServiceReady || latencyTestingApiIds.includes(api.id)
                                }
                                onClick={() => void runApiModelLatencyTest(api)}
                              >
                                <RefreshCw
                                  className={
                                    latencyTestingApiIds.includes(api.id)
                                      ? "h-3.5 w-3.5 animate-spin"
                                      : "h-3.5 w-3.5"
                                  }
                                />
                                测响
                              </Button>
                              <Button
                                variant="ghost"
                                size="sm"
                                className="h-7 gap-1.5 px-2 text-xs text-red-500 hover:text-red-600"
                                disabled={!latencyTestingApiIds.includes(api.id)}
                                onClick={() => cancelLatencyTests()}
                              >
                                取消测响
                              </Button>
                            </div>
                            {apiModelLatencies[api.id]?.testedAt ? (
                              <p className="text-[10px] text-muted-foreground">
                                {formatTsFromSeconds(
                                  apiModelLatencies[api.id].testedAt,
                                  t("未知时间"),
                                )}
                              </p>
                            ) : null}
                          </div>
                        </TableCell>
                        <TableCell className="align-middle border-r border-border/90 bg-blue-50/35 px-2 dark:bg-slate-900/35">
                          <div className="flex flex-col items-center justify-center gap-1">
                            <Switch
                              className="scale-75"
                              checked={isEnabled}
                              disabled={
                                !isServiceReady || togglingApiId === api.id
                              }
                              onCheckedChange={(enabled) =>
                                toggleStatusMutation.mutate({ api, enabled })
                              }
                            />
                            <span className="text-[10px] font-medium text-muted-foreground">
                              {isEnabled ? t("启用") : t("禁用")}
                            </span>
                          </div>
                        </TableCell>
                        <TableCell className="bg-slate-50/80 dark:bg-slate-950/25">
                          <div className="flex items-center justify-center gap-2">
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-8 w-8 text-muted-foreground transition-colors hover:text-primary"
                              disabled={!isServiceReady}
                              onClick={() => openEditModal(api.id)}
                              title={t("编辑配置")}
                            >
                              <Settings2 className="h-4 w-4" />
                            </Button>
                            <DropdownMenu>
                              <DropdownMenuTrigger>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-8 w-8"
                                  render={<span />}
                                  nativeButton={false}
                                  disabled={!isServiceReady}
                                >
                                  <MoreVertical className="h-4 w-4" />
                                </Button>
                              </DropdownMenuTrigger>
                              <DropdownMenuContent align="end">
                                <DropdownMenuItem
                                  className="gap-2"
                                  disabled={!isServiceReady}
                                  onClick={() => openEditModal(api.id)}
                                >
                                  {t("编辑聚合 API")}
                                </DropdownMenuItem>
                                <DropdownMenuItem
                                  className="gap-2"
                                  disabled={!isServiceReady}
                                  onClick={() => openBindingDialog(api)}
                                >
                                  <Boxes className="h-4 w-4" /> 绑定模型
                                </DropdownMenuItem>
                                <DropdownMenuItem
                                  className="gap-2"
                                  disabled={
                                    !isServiceReady || prioritizeMutation.isPending
                                  }
                                  onClick={() => prioritizeMutation.mutate(api)}
                                >
                                  <ArrowUp className="h-4 w-4" /> {t("设为优先")}
                                </DropdownMenuItem>
                                <DropdownMenuItem
                                  className="gap-2 text-red-500"
                                  disabled={!isServiceReady}
                                  onClick={() => setDeleteId(api.id)}
                                >
                                  <Trash2 className="h-4 w-4" /> {t("删除聚合 API")}
                                </DropdownMenuItem>
                              </DropdownMenuContent>
                            </DropdownMenu>
                          </div>
                        </TableCell>
                      </TableRow>
                    );
                      }),
                    ];
                  })
                )}
              </TableBody>
            </Table>
            </div>
          </CardContent>
        </Card>
      </div>

      <AggregateApiModal
        open={modalOpen}
        onOpenChange={setModalOpen}
        aggregateApi={editingApi}
        templateApi={templateApi}
        defaultSort={defaultCreateSort}
        defaultPool={editingApi?.pool || templateApi?.pool || poolFilter}
        defaultSupplierName={
          !editingApi && !templateApi && poolFilter === "wool"
            ? defaultWoolSupplierName
            : undefined
        }
      />

      <Dialog
        open={Boolean(bindingDialog)}
        onOpenChange={(open) => {
          if (!open) setBindingDialog(null);
        }}
      >
        <DialogContent className="glass-card max-h-[calc(100vh-2rem)] overflow-y-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>绑定模型</DialogTitle>
          </DialogHeader>
          {bindingDialog ? (() => {
            const api = aggregateApiById.get(bindingDialog.apiId);
            const targetApis = bindingDialog.apiIds
              .map((id) => aggregateApiById.get(id))
              .filter((item): item is AggregateApi => Boolean(item));
            const currentModels = Array.from(
              new Set(bindingDialog.apiIds.flatMap((id) => bindingImpactModelsForApi(id))),
            );
            const selectedCandidates = bindingDialog.probeCandidates.filter((candidate) =>
              bindingDialog.selectedCandidateIds.includes(candidate.id),
            );
            return api ? (
              <div className="space-y-4">
                <div className="rounded-lg border border-border bg-muted/40 p-3">
                  <div className="font-medium">{api.supplierName || api.url}</div>
                  <div className="mt-1 break-all font-mono text-xs text-muted-foreground">
                    {api.url}
                  </div>
                  <div className="mt-2 text-xs text-muted-foreground">
                    本次会写入 {targetApis.length} 个 key；组级绑定会给组内每个 key 建立相同模型绑定。
                  </div>
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {currentModels.length > 0 ? (
                      currentModels.slice(0, 8).map((model) => (
                        <Badge key={model} variant="secondary">
                          {model}
                        </Badge>
                      ))
                    ) : (
                      <Badge variant="secondary">当前未绑定模型</Badge>
                    )}
                    {currentModels.length > 8 ? (
                      <Badge variant="secondary">+{currentModels.length - 8}</Badge>
                    ) : null}
                  </div>
                </div>

                <div className="space-y-2">
                  <label className="text-xs font-medium text-muted-foreground">
                    手动输入模型，多个模型可用空格、逗号或换行分隔
                  </label>
                  <textarea
                    className="min-h-20 w-full rounded-md border border-border bg-background px-3 py-2 font-mono text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    value={bindingDialog.modelDraft}
                    onChange={(event) =>
                      setBindingDialog((current) =>
                        current
                          ? { ...current, modelDraft: event.target.value }
                          : current,
                      )
                    }
                    placeholder="glm-5.1 kimi-k2.6 gpt-5.5"
                  />
                  {knownRouteModels.length > 0 ? (
                    <div className="flex flex-wrap gap-1.5">
                      {knownRouteModels.slice(0, 10).map((model) => (
                        <Button
                          key={model}
                          type="button"
                          variant="outline"
                          size="sm"
                          className="h-7 px-2 text-xs"
                          onClick={() =>
                            setBindingDialog((current) =>
                              current
                                ? {
                                    ...current,
                                    modelDraft: Array.from(
                                      new Set([
                                        ...current.modelDraft
                                          .split(/[\n,，;；\s]+/)
                                          .map((item) => item.trim())
                                          .filter(Boolean),
                                        model,
                                      ]),
                                    ).join(" "),
                                  }
                                : current,
                            )
                          }
                        >
                          {model}
                        </Button>
                      ))}
                    </div>
                  ) : null}
                </div>

                <div className="space-y-2 rounded-lg border border-border bg-background/50 p-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div>
                      <div className="text-sm font-medium">探测候选模型</div>
                      <div className="text-xs text-muted-foreground">
                        先探测再勾选，可避免把不需要的模型全部写入路由表。
                      </div>
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      className="gap-1.5"
                      disabled={!isServiceReady || bindingDialog.probing}
                      onClick={() => void runBindingProbe(api)}
                    >
                      <RefreshCw
                        className={cn(
                          "h-3.5 w-3.5",
                          bindingDialog.probing && "animate-spin",
                        )}
                      />
                      探测
                    </Button>
                  </div>
                  {bindingDialog.probeCandidates.length > 0 ? (
                    <div className="grid gap-2 md:grid-cols-2">
                      {bindingDialog.probeCandidates.map((candidate) => (
                        <label
                          key={candidate.id}
                          className="flex cursor-pointer items-start gap-2 rounded-md border border-border bg-card/70 p-2"
                        >
                          <Checkbox
                            checked={bindingDialog.selectedCandidateIds.includes(candidate.id)}
                            onCheckedChange={(checked) =>
                              setBindingDialog((current) => {
                                if (!current) return current;
                                const next = new Set(current.selectedCandidateIds);
                                if (checked) next.add(candidate.id);
                                else next.delete(candidate.id);
                                return {
                                  ...current,
                                  selectedCandidateIds: Array.from(next),
                                };
                              })
                            }
                          />
                          <div className="min-w-0">
                            <div className="truncate font-mono text-xs">{candidate.model}</div>
                            <div className="mt-1 flex flex-wrap gap-1">
                              {candidate.supportsResponses ? (
                                <Badge variant="secondary">responses</Badge>
                              ) : null}
                              {candidate.supportsChatCompletions ? (
                                <Badge variant="secondary">chat</Badge>
                              ) : null}
                              {candidate.requiresAdapter ? (
                                <Badge className="border-amber-500/20 bg-amber-500/10 text-amber-500">
                                  转换
                                </Badge>
                              ) : null}
                            </div>
                          </div>
                        </label>
                      ))}
                    </div>
                  ) : (
                    <div className="rounded-md border border-dashed border-border p-4 text-sm text-muted-foreground">
                      暂无候选模型。可以先点“探测”，或直接手动输入模型名。
                    </div>
                  )}
                </div>

                <DialogFooter>
                  <Button
                    variant="outline"
                    onClick={() => setBindingDialog(null)}
                  >
                    {t("取消")}
                  </Button>
                  <Button
                    disabled={!isServiceReady || saveRouteBindingsMutation.isPending}
                    onClick={() =>
                      saveRouteBindingsMutation.mutate({
                        apis: targetApis,
                        modelDraft: bindingDialog.modelDraft,
                        candidates: selectedCandidates,
                      })
                    }
                  >
                    保存绑定
                  </Button>
                </DialogFooter>
              </div>
            ) : null;
          })() : null}
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={Boolean(deleteId)}
        onOpenChange={(open) => !open && setDeleteId(null)}
        title={t("删除聚合 API")}
        description={
          deleteTargetModels.length > 0
            ? `将删除 ${deleteTargetApi?.supplierName || deleteTargetApi?.url || deleteId}，并自动解除 ${deleteTargetModels.length} 条模型绑定：${deleteTargetModels.slice(0, 6).join("、")}${deleteTargetModels.length > 6 ? "..." : ""}。禁用 API 不会解除绑定；只有删除才会解除。`
            : `将删除 ${deleteTargetApi?.supplierName || deleteTargetApi?.url || deleteId}。当前没有模型绑定会被解除。`
        }
        confirmText={t("删除")}
        cancelText={t("取消")}
        confirmVariant="destructive"
        onConfirm={() => {
          if (!deleteId) return;
          deleteMutation.mutate(deleteId);
          setDeleteId(null);
        }}
      />

      <ConfirmDialog
        open={confirmBulkDeleteOpen}
        onOpenChange={(open) => !open && setConfirmBulkDeleteOpen(false)}
        title="删除所选聚合 API"
        description={`将删除已选 ${selectedApis.length} 个聚合 API，并自动解除 ${selectedDeleteBindingCount} 条模型绑定。禁用 API 不会解除绑定；只有删除才会解除。此操作不会因为单项失败中断，失败项会汇总提示。`}
        confirmText="删除所选"
        cancelText={t("取消")}
        confirmVariant="destructive"
        onConfirm={() => void deleteSelectedApis()}
      />
    </div>
  );
}

function normalizeAggregateApiUrl(value: string): string {
  return String(value || "").trim().replace(/\/+$/, "").toLowerCase();
}
