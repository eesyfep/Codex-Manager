import type { Account, AccountUsage, UsageAggregateSummary } from "@/types/account";
import type {
  ApiKey,
  DashboardDailyTokenUsageBucket,
  DashboardTokenUsage,
} from "@/types/api-key";
import type { ModelCatalog } from "@/types/model";
import type { RequestLog, RequestLogTodaySummary } from "@/types/request-log";

export interface StartupSnapshot {
  accounts: Account[];
  usageSnapshots: AccountUsage[];
  usageAggregateSummary: UsageAggregateSummary;
  apiKeys: ApiKey[];
  apiModels: ModelCatalog;
  manualPreferredAccountId: string;
  requestLogTodaySummary: RequestLogTodaySummary;
  requestLogs: RequestLog[];
  dashboardTokenUsage: DashboardTokenUsage[];
  dashboardDailyTokenUsage: DashboardDailyTokenUsageBucket[];
}
