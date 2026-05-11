'use client';

import {
  Activity,
  BrainCircuit,
  CheckCircle2,
  CalendarDays,
  Database,
  DollarSign,
  PieChart,
  Users,
  XCircle,
  Zap,
  type LucideIcon,
} from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Progress } from '@/components/ui/progress';
import { Skeleton } from '@/components/ui/skeleton';
import { useDashboardStats } from '@/hooks/useDashboardStats';
import { usePageTransitionReady } from '@/hooks/usePageTransitionReady';
import { useI18n } from '@/lib/i18n/provider';
import { formatPricing, resolveModelPricing } from '@/lib/pricing/model-pricing';
import { cn } from '@/lib/utils';
import { formatCompactNumber } from '@/lib/utils/usage';
import type { DashboardDailyTokenUsageBucket } from '@/types/api-key';

interface StatProgressCardProps {
  title: string;
  value: number;
  total: number;
  icon: LucideIcon;
  color: string;
  sub: string;
}

type ChartGranularity = 'day' | 'week' | 'month';
type TokenChartBucket = ReturnType<typeof buildTokenChartData>[number];

const MODEL_SERIES_COLORS = ['#8fa7a3', '#c4a69a', '#a9a8c8', '#c9b27c', '#91a7c6', '#b88f96', '#9db58f', '#b7a6c7'];
const TOKEN_TREND_STROKE = '#8fa7a3';
const COST_TREND_STROKE = '#c4a69a';
const TOKEN_TREND_FILL_TOP = 'rgba(143,167,163,0.3)';
const TOKEN_TREND_FILL_BOTTOM = 'rgba(143,167,163,0.02)';

function formatPercent(value: number | null | undefined): string {
  return value == null ? '--' : `${Math.max(0, Math.round(value))}%`;
}

function formatCompactTokenAmount(value: number | null | undefined): string {
  const normalized = typeof value === 'number' && Number.isFinite(value) ? Math.max(0, value) : 0;
  if (normalized < 1000) {
    return normalized.toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  }
  return formatCompactNumber(normalized, '0.00', 2, true);
}

function periodStartTs(dayStartTs: number, granularity: ChartGranularity): number {
  const date = new Date(dayStartTs * 1000);
  date.setHours(0, 0, 0, 0);
  if (granularity === 'week') {
    const day = date.getDay();
    const diff = day === 0 ? -6 : 1 - day;
    date.setDate(date.getDate() + diff);
  } else if (granularity === 'month') {
    date.setDate(1);
  }
  return Math.floor(date.getTime() / 1000);
}

function formatPeriodLabel(dayStartTs: number, granularity: ChartGranularity): string {
  const date = new Date(dayStartTs * 1000);
  if (granularity === 'month') {
    return date.toLocaleDateString('zh-CN', { year: '2-digit', month: '2-digit' });
  }
  return date.toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit' });
}

function seriesKey(item: DashboardDailyTokenUsageBucket): string {
  return item.sourceKey || item.model || 'unknown';
}

function seriesLabel(item: DashboardDailyTokenUsageBucket): string {
  return item.model || item.sourceLabel || '未知模型';
}

function buildTokenChartData(items: DashboardDailyTokenUsageBucket[], granularity: ChartGranularity) {
  const periods = new Map<string, {
    periodStartTs: number;
    sourceKey: string;
    sourceLabel: string;
    model: string | null;
    totalTokens: number;
    estimatedCostUsd: number;
    requestCount: number;
    inputTokens: number;
    billableInputTokens: number;
    cachedInputTokens: number;
    outputTokens: number;
    reasoningOutputTokens: number;
    sources: DashboardDailyTokenUsageBucket[];
  }>();

  for (const item of items) {
    const periodStart = periodStartTs(item.dayStartTs, granularity);
    const key = `${periodStart}\u001f${seriesKey(item)}`;
    const period = periods.get(key) ?? {
      periodStartTs: periodStart,
      sourceKey: seriesKey(item),
      sourceLabel: seriesLabel(item),
      model: item.model,
      totalTokens: 0,
      estimatedCostUsd: 0,
      requestCount: 0,
      inputTokens: 0,
      billableInputTokens: 0,
      cachedInputTokens: 0,
      outputTokens: 0,
      reasoningOutputTokens: 0,
      sources: [],
    };
    period.totalTokens += item.totalTokens;
    period.estimatedCostUsd += item.estimatedCostUsd;
    period.requestCount += item.requestCount;
    period.inputTokens += item.inputTokens;
    period.billableInputTokens += item.billableInputTokens;
    period.cachedInputTokens += item.cachedInputTokens;
    period.outputTokens += item.outputTokens;
    period.reasoningOutputTokens += item.reasoningOutputTokens;
    period.sources.push(item);
    periods.set(key, period);
  }

  const buckets = new Map<number, {
    periodStartTs: number;
    totalTokens: number;
    estimatedCostUsd: number;
    requestCount: number;
    inputTokens: number;
    cachedInputTokens: number;
    outputTokens: number;
    reasoningOutputTokens: number;
    segments: Array<{
      sourceKey: string;
      sourceLabel: string;
      model: string | null;
      totalTokens: number;
      estimatedCostUsd: number;
      requestCount: number;
      billableInputTokens: number;
      cachedInputTokens: number;
      outputTokens: number;
      reasoningOutputTokens: number;
    }>;
  }>();

  for (const period of periods.values()) {
    const bucket = buckets.get(period.periodStartTs) ?? {
      periodStartTs: period.periodStartTs,
      totalTokens: 0,
      estimatedCostUsd: 0,
      requestCount: 0,
      inputTokens: 0,
      cachedInputTokens: 0,
      outputTokens: 0,
      reasoningOutputTokens: 0,
      segments: [],
    };
    bucket.totalTokens += period.totalTokens;
    bucket.estimatedCostUsd += period.estimatedCostUsd;
    bucket.requestCount += period.requestCount;
    bucket.inputTokens += period.inputTokens;
    bucket.cachedInputTokens += period.cachedInputTokens;
    bucket.outputTokens += period.outputTokens;
    bucket.reasoningOutputTokens += period.reasoningOutputTokens;
    bucket.segments.push({
      sourceKey: period.sourceKey,
      sourceLabel: period.sourceLabel,
      model: period.model,
      totalTokens: period.totalTokens,
      estimatedCostUsd: period.estimatedCostUsd,
      requestCount: period.requestCount,
      billableInputTokens: period.billableInputTokens,
      cachedInputTokens: period.cachedInputTokens,
      outputTokens: period.outputTokens,
      reasoningOutputTokens: period.reasoningOutputTokens,
    });
    buckets.set(period.periodStartTs, bucket);
  }

  return Array.from(buckets.values())
    .sort((left, right) => left.periodStartTs - right.periodStartTs)
    .map((bucket) => ({
      ...bucket,
      segments: bucket.segments.sort(
        (left, right) =>
          right.estimatedCostUsd - left.estimatedCostUsd || right.totalTokens - left.totalTokens,
      ),
    }));
}

function formatMoneyAxis(value: number): string {
  if (value >= 1000) return `$${(value / 1000).toFixed(1)}K`;
  if (value >= 10) return `$${value.toFixed(0)}`;
  return `$${value.toFixed(2)}`;
}

function formatTokenAxis(value: number): string {
  if (value >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(1)}B`;
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1000) return `${(value / 1000).toFixed(0)}K`;
  return value.toFixed(0);
}

function formatTimelineRange(items: ReturnType<typeof buildTokenChartData>, granularity: ChartGranularity): string {
  if (items.length === 0) return '--';
  return `${formatPeriodLabel(items[0].periodStartTs, granularity)} - ${formatPeriodLabel(items[items.length - 1].periodStartTs, granularity)}`;
}

function buildLineChartPath(points: Array<{ x: number; y: number }>): string {
  if (points.length === 0) return '';
  return points.map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.x.toFixed(2)} ${point.y.toFixed(2)}`).join(' ');
}

function buildAreaChartPath(points: Array<{ x: number; y: number }>, baselineY: number): string {
  if (points.length === 0) return '';
  const linePath = buildLineChartPath(points);
  const first = points[0];
  const last = points[points.length - 1];
  return `${linePath} L ${last.x.toFixed(2)} ${baselineY.toFixed(2)} L ${first.x.toFixed(2)} ${baselineY.toFixed(2)} Z`;
}

function buildTokenHoverSummary(day: TokenChartBucket, granularity: ChartGranularity): string {
  return [
    `${formatPeriodLabel(day.periodStartTs, granularity)} · ${formatCompactTokenAmount(day.totalTokens)} tokens`,
    `费用 $${day.estimatedCostUsd.toFixed(4)} · 请求 ${day.requestCount}`,
    ...day.segments.map(
      (source) =>
        `${source.sourceLabel} · ${formatCompactTokenAmount(source.totalTokens)} tokens · $${source.estimatedCostUsd.toFixed(4)} · ${source.requestCount} 次`,
    ),
  ].join('\n');
}

function buildTokenHoverColor(index: number): string {
  return MODEL_SERIES_COLORS[index % MODEL_SERIES_COLORS.length];
}

function quotaTrackClass(tone: 'green' | 'blue') {
  return tone === 'blue' ? 'bg-blue-500/20' : 'bg-green-500/20';
}

function quotaIndicatorClass(tone: 'green' | 'blue') {
  return tone === 'blue' ? 'bg-blue-500' : 'bg-green-500';
}

function StatProgressCard({ title, value, total, icon: Icon, color, sub }: StatProgressCardProps) {
  const { t } = useI18n();
  const percentage = total > 0 ? Math.min(Math.round((value / total) * 100), 100) : 0;

  return (
    <Card className="glass-card overflow-hidden border border-border/70 bg-card/80 shadow-md backdrop-blur-md transition-all hover:shadow-lg">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
        <Icon className={cn('h-4 w-4', color)} />
      </CardHeader>
      <CardContent className="space-y-3">
        <div>
          <div className="text-2xl font-bold">{value}</div>
          <p className="mt-1 text-[10px] text-muted-foreground">{sub}</p>
        </div>
        <div className="space-y-1">
          <div className="flex items-center justify-between text-[10px]">
            <span className="text-muted-foreground">{t('占比')}</span>
            <span className="font-mono font-medium">{percentage}%</span>
          </div>
          <Progress value={percentage} className="h-1.5" />
        </div>
      </CardContent>
    </Card>
  );
}

export default function DashboardPage() {
  const { t } = useI18n();
  const [chartGranularity, setChartGranularity] = useState<ChartGranularity>('day');
  const [hoveredTokenIndex, setHoveredTokenIndex] = useState<number | null>(null);
  const [hoveredModelIndex, setHoveredModelIndex] = useState<number | null>(null);
  const tokenTrendScrollRef = useRef<HTMLDivElement | null>(null);
  const { stats, dashboardDailyTokenUsage, requestLogs, isLoading, isServiceReady } = useDashboardStats();
  usePageTransitionReady('/', !isServiceReady || !isLoading);

  const poolPrimary = stats.poolRemain?.primary ?? 0;
  const poolSecondary = stats.poolRemain?.secondary ?? 0;
  const tokenChartData = useMemo(() => buildTokenChartData(dashboardDailyTokenUsage, chartGranularity), [chartGranularity, dashboardDailyTokenUsage]);
  const chartMaxCost = Math.max(1, ...tokenChartData.map((day) => day.estimatedCostUsd));
  const chartMaxTokens = Math.max(1, ...tokenChartData.map((day) => day.totalTokens));
  const latestTokenPointIndex = tokenChartData.length > 0 ? tokenChartData.length - 1 : -1;
  const trendChartPoints = useMemo(() => {
    const width = 900;
    const leftPadding = 24;
    const rightPadding = 24;
    const topPadding = 24;
    const plotHeight = 240;
    const plotWidth = width - leftPadding - rightPadding;
    const span = Math.max(1, tokenChartData.length - 1);
    return tokenChartData.map((day, index) => {
      const x = tokenChartData.length <= 1
        ? width / 2
        : leftPadding + (index / span) * plotWidth;
      const tokenY = topPadding + (1 - day.totalTokens / chartMaxTokens) * plotHeight;
      const costY = topPadding + (1 - day.estimatedCostUsd / chartMaxCost) * plotHeight;
      return { ...day, x, y: tokenY, tokenY, costY };
    });
  }, [chartMaxCost, chartMaxTokens, tokenChartData]);
  const trendLinePath = useMemo(() => buildLineChartPath(trendChartPoints), [trendChartPoints]);
  const trendAreaPath = useMemo(() => buildAreaChartPath(trendChartPoints, 264), [trendChartPoints]);
  const costTrendLinePath = useMemo(
    () => buildLineChartPath(trendChartPoints.map((point) => ({ x: point.x, y: point.costY }))),
    [trendChartPoints],
  );
  const activeTokenBucket =
    hoveredTokenIndex == null ? null : tokenChartData[hoveredTokenIndex] ?? null;
  const modelSeries = useMemo(() => {
    const series = new Map<string, {
      key: string;
      label: string;
      color: string;
      totalTokens: number;
      estimatedCostUsd: number;
      requestCount: number;
      pricing: ReturnType<typeof resolveModelPricing>;
    }>();
    for (const item of dashboardDailyTokenUsage) {
      const key = seriesKey(item);
      const existing = series.get(key) ?? {
        key,
        label: seriesLabel(item),
        color: MODEL_SERIES_COLORS[series.size % MODEL_SERIES_COLORS.length],
        totalTokens: 0,
        estimatedCostUsd: 0,
        requestCount: 0,
        pricing: resolveModelPricing(item.model || item.sourceLabel),
      };
      existing.totalTokens += item.totalTokens;
      existing.estimatedCostUsd += item.estimatedCostUsd;
      existing.requestCount += item.requestCount;
      series.set(key, existing);
    }
    return Array.from(series.values()).sort((left, right) => right.estimatedCostUsd - left.estimatedCostUsd || right.totalTokens - left.totalTokens);
  }, [dashboardDailyTokenUsage]);
  const visibleModelSeries = useMemo(() => modelSeries.slice(0, 6), [modelSeries]);
  const visibleModelCostTotal = Math.max(
    1,
    visibleModelSeries.reduce((sum, item) => sum + item.estimatedCostUsd, 0),
  );
  const activeModelSeries =
    hoveredModelIndex == null ? null : visibleModelSeries[hoveredModelIndex] ?? null;
  const modelTrendSeries = useMemo(() => {
    const periodIndex = new Map(tokenChartData.map((bucket, index) => [bucket.periodStartTs, index]));
    return visibleModelSeries.slice(0, 4).map((series) => {
      const values = new Array(tokenChartData.length).fill(0) as number[];
      for (const item of dashboardDailyTokenUsage) {
        const period = periodStartTs(item.dayStartTs, chartGranularity);
        const index = periodIndex.get(period);
        if (index == null) continue;
        if (seriesKey(item) === series.key) {
          values[index] += item.totalTokens;
        }
      }
      const points = values.map((value, index) => ({
        x: trendChartPoints[index]?.x ?? 0,
        y: 24 + (1 - value / chartMaxTokens) * 240,
      }));
      return {
        key: series.key,
        label: series.label,
        color: series.color,
        path: buildLineChartPath(points),
      };
    });
  }, [chartGranularity, chartMaxTokens, dashboardDailyTokenUsage, tokenChartData, trendChartPoints, visibleModelSeries]);

  useEffect(() => {
    const element = tokenTrendScrollRef.current;
    if (!element || tokenChartData.length === 0) return;
    element.scrollLeft = element.scrollWidth - element.clientWidth;
  }, [chartGranularity, tokenChartData.length]);

  return (
    <div className="space-y-6 animate-in fade-in duration-700">
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        {isLoading ? (
          Array.from({ length: 4 }).map((_, index) => <Skeleton key={index} className="h-36 w-full rounded-2xl" />)
        ) : (
          <>
            <Card className="glass-card overflow-hidden border border-border/70 bg-card/80 shadow-md backdrop-blur-md transition-all hover:shadow-lg">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">{t('总账号数')}</CardTitle>
                <Users className="h-4 w-4 text-blue-500" />
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{stats.total}</div>
                <p className="mt-1 text-[10px] text-muted-foreground">{t('池中所有配置账号')}</p>
                <div className="mt-4 flex w-fit items-center gap-2 rounded-full bg-blue-500/10 px-2 py-0.5 text-[10px] text-blue-600 dark:text-blue-400">
                  <Activity className="h-3 w-3" />
                  {t('最近日志')} {requestLogs.length} {t('条')}
                </div>
              </CardContent>
            </Card>

            <StatProgressCard title={t('可用账号')} value={stats.available} total={stats.total} icon={CheckCircle2} color="text-green-500" sub={t('当前健康可调用的账号')} />
            <StatProgressCard title={t('不可用账号')} value={stats.unavailable} total={stats.total} icon={XCircle} color="text-red-500" sub={t('额度耗尽或授权失效')} />

            <Card className="overflow-hidden border border-border/70 bg-primary/10 shadow-md backdrop-blur-md transition-all hover:shadow-lg">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium text-primary">{t('账号池剩余')}</CardTitle>
                <PieChart className="h-4 w-4 text-primary" />
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="space-y-1.5">
                  <div className="flex items-center justify-between text-[10px]">
                    <span className="text-muted-foreground">{t('5小时内')}</span>
                    <span className="font-bold">{formatPercent(stats.poolRemain?.primary)}</span>
                  </div>
                  <Progress value={poolPrimary} trackClassName={quotaTrackClass('green')} indicatorClassName={quotaIndicatorClass('green')} />
                </div>
                <div className="space-y-1.5">
                  <div className="flex items-center justify-between text-[10px]">
                    <span className="text-muted-foreground">{t('7天内')}</span>
                    <span className="font-bold">{formatPercent(stats.poolRemain?.secondary)}</span>
                  </div>
                  <Progress value={poolSecondary} trackClassName={quotaTrackClass('blue')} indicatorClassName={quotaIndicatorClass('blue')} />
                </div>
              </CardContent>
            </Card>
          </>
        )}
      </div>

      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        {[
          { title: t('今日Token'), value: formatCompactTokenAmount(stats.todayTokens), icon: Zap, color: 'text-yellow-500', sub: t('输入 + 输出合计') },
          { title: t('缓存Token'), value: formatCompactTokenAmount(stats.cachedTokens), icon: Database, color: 'text-indigo-500', sub: t('上下文缓存命中') },
          { title: t('推理Token'), value: formatCompactTokenAmount(stats.reasoningTokens), icon: BrainCircuit, color: 'text-purple-500', sub: t('大模型思考过程') },
          { title: t('预计费用'), value: `$${Number(stats.todayCost || 0).toFixed(2)}`, icon: DollarSign, color: 'text-emerald-500', sub: t('按官价估算') },
        ].map((card) => (
          isLoading ? (
            <Skeleton key={card.title} className="h-32 w-full rounded-2xl" />
          ) : (
            <Card key={card.title} className="glass-card overflow-hidden border border-border/70 bg-card/80 shadow-md backdrop-blur-md transition-all hover:shadow-lg">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">{card.title}</CardTitle>
                <card.icon className={cn('h-4 w-4', card.color)} />
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{card.value}</div>
                <p className="mt-1 text-[10px] text-muted-foreground">{card.sub}</p>
              </CardContent>
            </Card>
          )
        ))}
      </div>

      <div className="grid items-start gap-6 xl:grid-cols-[minmax(420px,0.9fr)_minmax(560px,1.25fr)]">
        <Card className="overflow-hidden border border-border/70 bg-card/90 shadow-lg">
          <CardHeader className="pb-3">
            <CardTitle className="flex items-center gap-2 text-base font-semibold">
              <PieChart className="h-4 w-4 text-primary" />
              模型分布
            </CardTitle>
            <p className="mt-1 text-xs text-muted-foreground">按历史费用和 token 汇总，信息放到底部，不再并排挤压。</p>
          </CardHeader>
          <CardContent className="space-y-5">
            <div className="space-y-5">
              <div className="flex flex-col justify-between rounded-[28px] border border-border/70 bg-card/95 p-4 shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]">
                <div className="flex items-center justify-between text-[10px] uppercase tracking-[0.24em] text-muted-foreground">
                  <span>模型分布</span>
                  <span>{modelSeries.length} items</span>
                </div>
                <div
                  className="mt-4 flex min-h-[320px] items-center justify-center rounded-[24px] bg-background/70 px-3 py-4"
                  onMouseLeave={() => setHoveredModelIndex(null)}
                >
                  <div className="relative flex h-64 w-64 items-center justify-center">
                    <svg viewBox="0 0 240 240" className="h-full w-full -rotate-90">
                      {visibleModelSeries.map((series, index) => {
                        const previous = visibleModelSeries.slice(0, index).reduce((sum, item) => sum + item.estimatedCostUsd, 0);
                        const ratio = series.estimatedCostUsd / visibleModelCostTotal;
                        const dash = `${Math.max(0.5, ratio * 565)} 565`;
                        return (
                          <circle
                            key={series.key}
                            cx="120"
                            cy="120"
                            r="90"
                            fill="none"
                            stroke={series.color}
                            strokeWidth={hoveredModelIndex === index ? '38' : '32'}
                            strokeDasharray={dash}
                            strokeDashoffset={-(previous / visibleModelCostTotal) * 565}
                            strokeLinecap="butt"
                            className="cursor-help transition-all duration-200"
                            tabIndex={0}
                            role="button"
                            aria-label={`${series.label} ${formatCompactTokenAmount(series.totalTokens)} tokens $${series.estimatedCostUsd.toFixed(4)}`}
                            onMouseEnter={() => setHoveredModelIndex(index)}
                            onFocus={() => setHoveredModelIndex(index)}
                            onBlur={() => setHoveredModelIndex(null)}
                          />
                        );
                      })}
                      <circle cx="120" cy="120" r="58" className="fill-background stroke-border" strokeWidth="2" />
                    </svg>
                    <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center text-center">
                      <div className="text-xs font-medium text-muted-foreground">历史费用</div>
                      <div className="font-mono text-xl font-semibold text-foreground">
                        ${modelSeries.reduce((sum, item) => sum + item.estimatedCostUsd, 0).toFixed(2)}
                      </div>
                      <div className="mt-1 text-[10px] text-muted-foreground">{modelSeries.length} 个模型</div>
                    </div>
                    {activeModelSeries ? (
                      <div className="absolute left-1/2 top-4 w-56 -translate-x-1/2 rounded-2xl border border-border bg-card/95 p-3 text-left shadow-xl backdrop-blur-xl">
                        <div className="truncate text-xs font-semibold text-foreground">{activeModelSeries.label}</div>
                        <div className="mt-1 text-[10px] text-muted-foreground">{formatPricing(activeModelSeries.pricing)}</div>
                        <div className="mt-2 grid grid-cols-2 gap-2 text-[10px]">
                          <div className="rounded-lg bg-muted/35 px-2 py-1.5">
                            <div className="text-muted-foreground">token</div>
                            <div className="font-mono text-foreground">{formatCompactTokenAmount(activeModelSeries.totalTokens)}</div>
                          </div>
                          <div className="rounded-lg bg-muted/35 px-2 py-1.5">
                            <div className="text-muted-foreground">费用</div>
                            <div className="font-mono text-foreground">${activeModelSeries.estimatedCostUsd.toFixed(4)}</div>
                          </div>
                        </div>
                      </div>
                    ) : null}
                  </div>
                </div>
              </div>
              <div className="flex flex-col gap-2 rounded-[28px] border border-border/70 bg-card/95 p-4 shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]">
                <div className="flex items-center justify-between text-[10px] uppercase tracking-[0.24em] text-muted-foreground">
                  <span>模型信息</span>
                  <span>费用 / 请求 / token</span>
                </div>
                <div className="grid gap-2">
                  {modelSeries.slice(0, 8).map((series) => (
                    <div key={series.key} className="grid grid-cols-[minmax(0,1fr)_72px_92px_88px] items-center gap-2 rounded-xl border border-border/70 bg-muted/20 px-3 py-2 text-sm" title={`${series.label}\n${formatPricing(series.pricing)}`}>
                      <div className="flex min-w-0 items-center gap-2">
                        <span className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ backgroundColor: series.color }} />
                        <div className="min-w-0">
                          <div className="truncate font-medium">{series.label}</div>
                          <div className="truncate text-[10px] text-muted-foreground">{formatPricing(series.pricing)}</div>
                        </div>
                      </div>
                      <span className="text-right font-mono text-muted-foreground">{formatCompactNumber(series.requestCount)}</span>
                      <span className="text-right font-mono text-muted-foreground">{formatCompactTokenAmount(series.totalTokens)}</span>
                      <span className="text-right font-mono text-foreground">${series.estimatedCostUsd.toFixed(4)}</span>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card className="overflow-hidden border border-border/70 bg-card/90 shadow-lg">
          <CardHeader className="pb-3">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <CardTitle className="flex items-center gap-2 text-base font-semibold">
                  <CalendarDays className="h-4 w-4 text-primary" />
                  Token 使用趋势
                </CardTitle>
                <p className="mt-1 text-xs text-muted-foreground">参考你给的深色图风格，默认定位到最新日期，支持按日 / 周 / 月切换。</p>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                {(['day', 'week', 'month'] as const).map((mode) => (
                  <button
                    key={mode}
                    type="button"
                    onClick={() => setChartGranularity(mode)}
                    className={cn(
                      'rounded-full border px-3 py-1 text-xs transition-colors',
                      chartGranularity === mode
                        ? 'border-primary bg-primary/15 text-primary shadow-sm'
                        : 'border-border bg-muted/25 text-muted-foreground hover:bg-muted/45',
                    )}
                  >
                    {mode === 'day' ? '按日' : mode === 'week' ? '按周' : '按月'}
                  </button>
                ))}
              </div>
            </div>
            <div className="mt-2 text-[11px] text-muted-foreground">
              {formatTimelineRange(tokenChartData, chartGranularity)} · 总费用 ${tokenChartData.reduce((sum, day) => sum + day.estimatedCostUsd, 0).toFixed(2)} · 最新区间 {latestTokenPointIndex >= 0 ? formatPeriodLabel(tokenChartData[latestTokenPointIndex].periodStartTs, chartGranularity) : '--'}
            </div>
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <div className="space-y-3">
                {Array.from({ length: 5 }).map((_, index) => <Skeleton key={index} className="h-14 w-full rounded-xl" />)}
              </div>
            ) : tokenChartData.length > 0 ? (
              <div className="space-y-4 rounded-[28px] border border-border/70 bg-card/95 p-4 shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]">
                <div className="rounded-[24px] border border-border/70 bg-background/80 p-4 shadow-inner">
                  <div className="mb-3 flex items-center justify-between gap-3">
                    <div className="text-[11px] uppercase tracking-[0.22em] text-muted-foreground">
                      Token 使用来源
                    </div>
                  </div>
                  <div className="grid grid-cols-[54px_minmax(0,1fr)_54px] gap-2">
                    <div className="flex h-[236px] flex-col justify-between py-3 text-right font-mono text-[11px] text-muted-foreground">
                      <span>{formatTokenAxis(chartMaxTokens)}</span>
                      <span>{formatTokenAxis(chartMaxTokens * 0.75)}</span>
                      <span>{formatTokenAxis(chartMaxTokens * 0.5)}</span>
                      <span>{formatTokenAxis(chartMaxTokens * 0.25)}</span>
                      <span>0</span>
                    </div>
                    <div
                      className="relative rounded-2xl border border-border/70 bg-card/60 px-4 pb-3 pt-4 shadow-inner"
                      onMouseLeave={() => setHoveredTokenIndex(null)}
                    >
                      {activeTokenBucket ? (
                        <div className="pointer-events-none absolute right-4 top-4 z-10 w-[320px] rounded-2xl border border-border/70 bg-card/95 p-3 text-foreground shadow-2xl backdrop-blur-xl">
                          <div className="text-[10px] uppercase tracking-[0.2em] text-muted-foreground">Token 使用来源</div>
                          <div className="mt-1 text-sm font-semibold text-foreground">
                            {formatPeriodLabel(activeTokenBucket.periodStartTs, chartGranularity)}
                          </div>
                          <div className="mt-2 grid grid-cols-2 gap-2 text-[11px]">
                            <div className="rounded-xl border border-border/70 bg-muted/35 px-2.5 py-2">
                              <div className="text-muted-foreground">总 token</div>
                              <div className="mt-0.5 font-mono text-sm text-foreground">
                                {formatCompactTokenAmount(activeTokenBucket.totalTokens)}
                              </div>
                            </div>
                            <div className="rounded-xl border border-border/70 bg-muted/35 px-2.5 py-2">
                              <div className="text-muted-foreground">总费用</div>
                              <div className="mt-0.5 font-mono text-sm text-foreground">
                                ${activeTokenBucket.estimatedCostUsd.toFixed(4)}
                              </div>
                            </div>
                          </div>
                          <div className="mt-2 space-y-1.5">
                            {activeTokenBucket.segments.slice(0, 4).map((source, index) => (
                              <div
                                key={`${source.sourceKey}-${source.sourceLabel}-${index}`}
                                className="flex items-start gap-2 rounded-xl border border-border/70 bg-muted/25 px-2.5 py-2"
                              >
                                <span
                                  className="mt-1 h-2.5 w-2.5 shrink-0 rounded-full"
                                  style={{ backgroundColor: buildTokenHoverColor(index) }}
                                />
                                <div className="min-w-0 flex-1">
                                  <div className="flex items-center justify-between gap-2">
                                    <div className="min-w-0 truncate text-xs font-medium text-foreground">
                                      {source.sourceLabel}
                                    </div>
                                    <div className="font-mono text-[11px] text-muted-foreground">
                                      {formatCompactTokenAmount(source.totalTokens)}
                                    </div>
                                  </div>
                                  <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[10px] text-muted-foreground">
                                    <span>{source.model || '未知模型'}</span>
                                    <span>·</span>
                                    <span>${source.estimatedCostUsd.toFixed(4)}</span>
                                    <span>·</span>
                                    <span>{source.requestCount} 次</span>
                                  </div>
                                </div>
                              </div>
                            ))}
                            {activeTokenBucket.segments.length > 4 ? (
                              <div className="text-[10px] text-muted-foreground">
                                还有 {activeTokenBucket.segments.length - 4} 个来源
                              </div>
                            ) : null}
                          </div>
                        </div>
                      ) : null}
                      <div ref={tokenTrendScrollRef} className="subtle-scrollbar overflow-x-auto overflow-y-hidden pb-2">
                        <svg
                          viewBox="0 0 900 300"
                          preserveAspectRatio="none"
                          className="h-[236px] min-w-[860px] overflow-visible"
                          onMouseLeave={() => setHoveredTokenIndex(null)}
                        >
                          <defs>
                            <linearGradient id="tokenTrendFill" x1="0" x2="0" y1="0" y2="1">
                              <stop offset="0%" stopColor={TOKEN_TREND_FILL_TOP} />
                              <stop offset="100%" stopColor={TOKEN_TREND_FILL_BOTTOM} />
                            </linearGradient>
                          </defs>
                          {[0, 1, 2, 3, 4].map((line) => (
                            <line key={line} x1="0" x2="900" y1={24 + line * 60} y2={24 + line * 60} stroke="rgba(148,163,184,0.16)" strokeWidth="1" />
                          ))}
                          {trendAreaPath ? <path d={trendAreaPath} fill="url(#tokenTrendFill)" /> : null}
                          {modelTrendSeries.map((series) =>
                            series.path ? (
                              <path
                                key={series.key}
                                d={series.path}
                                fill="none"
                                stroke={series.color}
                                strokeWidth="1.8"
                                strokeLinecap="round"
                                strokeLinejoin="round"
                                opacity="0.58"
                              />
                            ) : null,
                          )}
                          {costTrendLinePath ? <path d={costTrendLinePath} fill="none" stroke={COST_TREND_STROKE} strokeWidth="2.4" strokeDasharray="7 7" strokeLinecap="round" strokeLinejoin="round" /> : null}
                          {trendLinePath ? <path d={trendLinePath} fill="none" stroke={TOKEN_TREND_STROKE} strokeWidth="3.4" strokeLinecap="round" strokeLinejoin="round" /> : null}
                          {trendChartPoints.map((point, index) => (
                            <g key={`${point.periodStartTs}-${index}`}>
                              {hoveredTokenIndex === index ? (
                                <circle
                                  cx={point.x}
                                  cy={point.y}
                                  r="10"
                                  fill={TOKEN_TREND_STROKE}
                                  opacity="0.14"
                                />
                              ) : null}
                              <circle
                                cx={point.x}
                                cy={point.y}
                                r={hoveredTokenIndex === index ? '4.2' : '2.8'}
                                fill="var(--card)"
                                stroke={TOKEN_TREND_STROKE}
                                strokeWidth={hoveredTokenIndex === index ? '2.4' : '1.6'}
                                className="cursor-help transition-all duration-150"
                                tabIndex={0}
                                role="button"
                                aria-label={buildTokenHoverSummary(tokenChartData[index], chartGranularity)}
                                onMouseEnter={() => setHoveredTokenIndex(index)}
                                onFocus={() => setHoveredTokenIndex(index)}
                                onBlur={() => setHoveredTokenIndex(null)}
                              />
                              <text x={point.x} y="286" textAnchor="middle" fill="currentColor" fontSize="10" className="fill-muted-foreground">{formatPeriodLabel(point.periodStartTs, chartGranularity)}</text>
                            </g>
                          ))}
                          <text x="24" y="18" textAnchor="start" fill={TOKEN_TREND_STROKE} fontSize="12">{formatCompactTokenAmount(tokenChartData[tokenChartData.length - 1]?.totalTokens ?? 0)}</text>
                          <text x="876" y="18" textAnchor="end" fill={COST_TREND_STROKE} fontSize="12">${tokenChartData[tokenChartData.length - 1]?.estimatedCostUsd.toFixed(2) ?? '0.00'}</text>
                        </svg>
                      </div>
                    </div>
                    <div className="flex h-[236px] flex-col justify-between py-3 text-left font-mono text-[11px] text-muted-foreground">
                      <span>{formatMoneyAxis(chartMaxCost)}</span>
                      <span>{formatMoneyAxis(chartMaxCost * 0.75)}</span>
                      <span>{formatMoneyAxis(chartMaxCost * 0.5)}</span>
                      <span>{formatMoneyAxis(chartMaxCost * 0.25)}</span>
                      <span>$0</span>
                    </div>
                  </div>
                </div>
                <div className="flex flex-wrap gap-2 text-xs">
                  <span className="inline-flex items-center gap-1.5 rounded-full border border-border bg-muted/40 px-2.5 py-1 text-muted-foreground">
                    <span className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ backgroundColor: TOKEN_TREND_STROKE }} />
                    <span>总 token</span>
                  </span>
                  <span className="inline-flex items-center gap-1.5 rounded-full border border-border bg-muted/40 px-2.5 py-1 text-muted-foreground">
                    <span className="h-0.5 w-4 shrink-0 rounded-full" style={{ backgroundColor: COST_TREND_STROKE }} />
                    <span>费用 USD</span>
                  </span>
                  {modelSeries.slice(0, 6).map((series) => (
                    <span key={series.key} className="inline-flex items-center gap-1.5 rounded-full border border-border bg-muted/40 px-2.5 py-1 text-muted-foreground" title={`${series.label}\n${formatPricing(series.pricing)}`}>
                      <span className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ backgroundColor: series.color }} />
                      <span className="max-w-28 truncate">{series.label}</span>
                      <span className="font-mono text-foreground">${series.estimatedCostUsd.toFixed(2)}</span>
                    </span>
                  ))}
                </div>
              </div>
            ) : (
              <div className="flex min-h-[160px] flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
                <div className="rounded-full bg-accent/30 p-4">
                  <Database className="h-8 w-8 opacity-30" />
                </div>
                <p>{isServiceReady ? t('暂无 token 与费用记录。') : t('正在等待服务连接。')}</p>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
