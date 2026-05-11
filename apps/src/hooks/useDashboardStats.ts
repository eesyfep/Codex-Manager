"use client";

import { useEffect, useRef } from "react";
import { useQuery } from "@tanstack/react-query";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useLocalDayRange } from "@/hooks/useLocalDayRange";
import {
  buildStartupSnapshotQueryKey,
  hasStartupSnapshotSignal,
  STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
  STARTUP_SNAPSHOT_STALE_TIME,
  STARTUP_SNAPSHOT_WARMUP_INTERVAL_MS,
  STARTUP_SNAPSHOT_WARMUP_TIMEOUT_MS,
} from "@/lib/api/startup-snapshot";
import { serviceClient } from "@/lib/api/service-client";
import { useAppStore } from "@/lib/store/useAppStore";
import { pickBestRecommendations, pickCurrentAccount } from "@/lib/utils/usage";
import type {
  DashboardDailyTokenUsageBucket,
  DashboardTokenUsage,
} from "@/types/api-key";
import type { RequestLog } from "@/types/request-log";

/**
 * 函数 `useDashboardStats`
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
export function useDashboardStats() {
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const localDayRange = useLocalDayRange();
  const isServiceReady = serviceStatus.connected;
  const isPageActive = useDesktopPageActive("/");
  const isSnapshotQueryEnabled = useDeferredDesktopActivation(
    isServiceReady && isPageActive,
  );
  const warmupStartedAtRef = useRef<number | null>(null);

  useEffect(() => {
    if (!isServiceReady || !isPageActive) {
      warmupStartedAtRef.current = null;
      return;
    }
    warmupStartedAtRef.current = Date.now();
  }, [isPageActive, isServiceReady, serviceStatus.addr]);

  const snapshotQuery = useQuery({
    queryKey: buildStartupSnapshotQueryKey(
      serviceStatus.addr,
      STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
      localDayRange.dayStartTs,
    ),
    queryFn: () =>
      serviceClient.getStartupSnapshot({
        requestLogLimit: STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
        dayStartTs: localDayRange.dayStartTs,
        dayEndTs: localDayRange.dayEndTs,
      }),
    enabled: isSnapshotQueryEnabled,
    retry: 1,
    staleTime: STARTUP_SNAPSHOT_STALE_TIME,
    refetchInterval: (query) => {
      if (!isServiceReady || !isPageActive) return false;
      if (typeof document !== "undefined" && document.visibilityState !== "visible") {
        return false;
      }
      const startedAt = warmupStartedAtRef.current;
      if (startedAt == null) return false;
      if (Date.now() - startedAt >= STARTUP_SNAPSHOT_WARMUP_TIMEOUT_MS) {
        warmupStartedAtRef.current = null;
        return false;
      }

      const snapshot = query.state.data;
      if (!snapshot || snapshot.accounts.length === 0) {
        return false;
      }

      return hasStartupSnapshotSignal(snapshot)
        ? false
        : STARTUP_SNAPSHOT_WARMUP_INTERVAL_MS;
    },
    refetchIntervalInBackground: false,
  });

  const data = snapshotQuery.data;
  const accounts = data?.accounts || [];
  const hasStartupSignal = hasStartupSnapshotSignal(data);
  const shouldWarmupPoll =
    isPageActive &&
    isServiceReady &&
    accounts.length > 0 &&
    !hasStartupSignal &&
    snapshotQuery.isFetching;
  const hasSnapshotData = Boolean(data);
  const totalAccounts = accounts.length;
  const availableAccounts = accounts.filter((item) => item.isAvailable).length;
  const unavailableAccounts = totalAccounts - availableAccounts;
  const currentAccount = pickCurrentAccount(
    accounts,
    data?.requestLogs || []
  );
  const recommendations = pickBestRecommendations(accounts);
  const dashboardTokenUsage =
    data?.dashboardTokenUsage?.length
      ? data.dashboardTokenUsage
      : buildDashboardTokenUsageFallback(data?.requestLogs || []);
  const dashboardDailyTokenUsage =
    data?.dashboardDailyTokenUsage?.length
      ? data.dashboardDailyTokenUsage
      : buildDashboardDailyTokenUsageFallback(data?.requestLogs || []);

  return {
    stats: {
      total: totalAccounts,
      available: availableAccounts,
      unavailable: unavailableAccounts,
      todayTokens: data?.requestLogTodaySummary.todayTokens || 0,
      cachedTokens: data?.requestLogTodaySummary.cachedInputTokens || 0,
      reasoningTokens: data?.requestLogTodaySummary.reasoningOutputTokens || 0,
      todayCost: data?.requestLogTodaySummary.estimatedCost || 0,
      poolRemain: {
        primary: data?.usageAggregateSummary.primaryRemainPercent ?? null,
        secondary: data?.usageAggregateSummary.secondaryRemainPercent ?? null,
        primaryKnownCount: data?.usageAggregateSummary.primaryKnownCount ?? 0,
        primaryBucketCount: data?.usageAggregateSummary.primaryBucketCount ?? 0,
        secondaryKnownCount: data?.usageAggregateSummary.secondaryKnownCount ?? 0,
        secondaryBucketCount: data?.usageAggregateSummary.secondaryBucketCount ?? 0,
      },
    },
    currentAccount,
    recommendations,
    requestLogs: data?.requestLogs || [],
    dashboardTokenUsage,
    dashboardDailyTokenUsage,
    isLoading:
      (!isServiceReady && !hasSnapshotData) ||
      (!isSnapshotQueryEnabled && !data) ||
      snapshotQuery.isPending ||
      shouldWarmupPoll,
    isSyncingSnapshot: shouldWarmupPoll,
    isServiceReady,
    isError: snapshotQuery.isError,
    error: snapshotQuery.error,
  };
}

function localDayStartTs(ts: number): number {
  const date = new Date(ts * 1000);
  date.setHours(0, 0, 0, 0);
  return Math.floor(date.getTime() / 1000);
}

function buildDashboardDailyTokenUsageFallback(
  logs: RequestLog[]
): DashboardDailyTokenUsageBucket[] {
  const grouped = new Map<string, DashboardDailyTokenUsageBucket>();
  for (const log of logs) {
    if (!log.createdAt) continue;
    const dayStartTs = localDayStartTs(log.createdAt);
    const sourceKey =
      log.aggregateApiUrl ||
      log.upstreamUrl ||
      log.keyId ||
      log.accountId ||
      "unknown";
    const sourceLabel =
      log.aggregateApiSupplierName ||
      log.aggregateApiUrl ||
      log.keyId ||
      log.accountId ||
      "未知来源";
    const key = `${dayStartTs}\u001f${sourceKey}`;
    const item =
      grouped.get(key) ??
      {
        dayStartTs,
        sourceKey,
        sourceLabel,
        model: log.model || null,
        billableInputTokens: 0,
        requestCount: 0,
        inputTokens: 0,
        cachedInputTokens: 0,
        outputTokens: 0,
        reasoningOutputTokens: 0,
        totalTokens: 0,
        estimatedCostUsd: 0,
      };
    item.requestCount += 1;
    item.inputTokens += Math.max(0, log.inputTokens || 0);
    item.cachedInputTokens += Math.max(0, log.cachedInputTokens || 0);
    item.billableInputTokens += Math.max(
      0,
      (log.inputTokens || 0) - (log.cachedInputTokens || 0)
    );
    item.outputTokens += Math.max(0, log.outputTokens || 0);
    item.reasoningOutputTokens += Math.max(0, log.reasoningOutputTokens || 0);
    item.totalTokens += Math.max(
      0,
      log.totalTokens ??
        Math.max(0, (log.inputTokens || 0) - (log.cachedInputTokens || 0)) +
          (log.outputTokens || 0)
    );
    item.estimatedCostUsd += Math.max(0, log.estimatedCostUsd || 0);
    grouped.set(key, item);
  }
  return Array.from(grouped.values()).sort(
    (left, right) =>
      left.dayStartTs - right.dayStartTs || right.totalTokens - left.totalTokens
  );
}

function buildDashboardTokenUsageFallback(logs: RequestLog[]): DashboardTokenUsage[] {
  const grouped = new Map<string, DashboardTokenUsage>();
  for (const log of logs) {
    const key = [
      log.keyId || "",
      log.accountId || "",
      log.initialAggregateApiId || "",
      log.aggregateApiUrl || "",
      log.model || "",
    ].join("\u001f");
    const item =
      grouped.get(key) ??
      {
        keyId: log.keyId || null,
        keyName: null,
        accountId: log.accountId || null,
        accountLabel: null,
        aggregateApiId: log.initialAggregateApiId || null,
        aggregateApiSupplierName: log.aggregateApiSupplierName,
        aggregateApiUrl: log.aggregateApiUrl,
        model: log.model || null,
        requestCount: 0,
        inputTokens: 0,
        cachedInputTokens: 0,
        outputTokens: 0,
        reasoningOutputTokens: 0,
        totalTokens: 0,
        estimatedCostUsd: 0,
        lastUsedAt: log.createdAt,
      };
    item.requestCount += 1;
    item.inputTokens += Math.max(0, log.inputTokens || 0);
    item.cachedInputTokens += Math.max(0, log.cachedInputTokens || 0);
    item.outputTokens += Math.max(0, log.outputTokens || 0);
    item.reasoningOutputTokens += Math.max(0, log.reasoningOutputTokens || 0);
    item.totalTokens += Math.max(
      0,
      log.totalTokens ??
        Math.max(0, (log.inputTokens || 0) - (log.cachedInputTokens || 0)) +
          (log.outputTokens || 0)
    );
    item.estimatedCostUsd += Math.max(0, log.estimatedCostUsd || 0);
    item.lastUsedAt = Math.max(item.lastUsedAt || 0, log.createdAt || 0);
    grouped.set(key, item);
  }
  return Array.from(grouped.values())
    .filter((item) => item.totalTokens > 0 || item.estimatedCostUsd > 0)
    .sort(
      (left, right) =>
        right.totalTokens - left.totalTokens ||
        right.estimatedCostUsd - left.estimatedCostUsd
    )
    .slice(0, 12);
}
