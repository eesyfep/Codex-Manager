"use client";

import { 
  LayoutDashboard, 
  Users, 
  Key, 
  Boxes,
  Database,
  Route,
  Puzzle,
  FileText, 
  Settings, 
  UserRound,
  ChevronLeft, 
  ChevronRight
} from "lucide-react";
import { cn } from "@/lib/utils";
import { buildStaticRouteUrl } from "@/lib/utils/static-routes";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/lib/store/useAppStore";
import { useI18n } from "@/lib/i18n/provider";
import {
  memo,
  useCallback,
  useMemo,
  type MouseEvent,
} from "react";

const NAV_ITEMS = [
  { label: "仪表盘", href: "/", icon: LayoutDashboard },
  { label: "账号管理", href: "/accounts", icon: Users },
  { label: "聚合API", href: "/aggregate-api", icon: Database },
  { label: "模型路由", href: "/model-router", icon: Route },
  { label: "平台密钥", href: "/apikeys", icon: Key },
  { label: "模型管理", href: "/models", icon: Boxes },
  { label: "插件中心", href: "/plugins", icon: Puzzle },
  { label: "请求日志", href: "/logs", icon: FileText },
  { label: "设置", href: "/settings", icon: Settings },
  { label: "关于作者", href: "/author", icon: UserRound },
];

const NavItem = memo(({
  item,
  isActive,
  isSidebarOpen,
  onNavigate,
  itemName,
}: {
  item: typeof NAV_ITEMS[0],
  isActive: boolean,
  isSidebarOpen: boolean,
  onNavigate: (href: string, event: MouseEvent<HTMLAnchorElement>) => void,
  itemName: string,
}) => (
  <a
    href={buildStaticRouteUrl(item.href)}
    onClick={(event) => onNavigate(item.href, event)}
    aria-current={isActive ? "page" : undefined}
    className={cn(
      "flex items-center gap-3 rounded-lg px-3 py-2 transition-all duration-200 hover:bg-accent hover:text-accent-foreground",
      isActive ? "bg-accent text-accent-foreground" : "text-muted-foreground"
    )}
  >
    <item.icon className="h-4 w-4 shrink-0" />
    {isSidebarOpen && <span className="text-sm truncate">{itemName}</span>}
  </a>
));

NavItem.displayName = "NavItem";

/**
 * 函数 `Sidebar`
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
export function Sidebar() {
  const { t } = useI18n();
  const {
    isSidebarOpen,
    toggleSidebar,
    openCodexCliGuide,
    currentShellPath,
    navigateShellPath,
  } = useAppStore();

  const handleNavigate = useCallback(
    (href: string, event: MouseEvent<HTMLAnchorElement>) => {
      if (
        event.defaultPrevented ||
        event.button !== 0 ||
        event.metaKey ||
        event.ctrlKey ||
        event.shiftKey ||
        event.altKey
      ) {
        return;
      }

      if (href === currentShellPath) {
        event.preventDefault();
        return;
      }

      event.preventDefault();
      navigateShellPath(href);
    },
    [currentShellPath, navigateShellPath],
  );

  const renderedItems = useMemo(() => 
    NAV_ITEMS.map((item) => (
      <NavItem 
        key={item.href} 
        item={item} 
        itemName={t(item.label)}
        isActive={item.href === currentShellPath} 
        isSidebarOpen={isSidebarOpen}
        onNavigate={handleNavigate}
      />
    )),
    [currentShellPath, handleNavigate, isSidebarOpen, t]
  );

  return (
    <div
      className={cn(
        "relative z-20 flex shrink-0 flex-col glass-sidebar transition-[width] duration-300 ease-in-out",
        isSidebarOpen ? "w-56" : "w-16"
      )}
    >
      <div className="flex h-16 items-center border-b px-4 shrink-0">
        <button
          type="button"
          onClick={openCodexCliGuide}
          title={t("重新打开 Codex CLI 引导")}
          aria-label={t("重新打开 Codex CLI 引导")}
          className="flex w-full items-center gap-2 overflow-hidden rounded-xl px-2 py-1.5 text-left transition-colors duration-200 hover:bg-accent/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/60"
        >
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <span className="text-sm font-bold">CM</span>
          </div>
          {isSidebarOpen && (
            <div className="flex flex-col overflow-hidden animate-in fade-in duration-300">
              <span className="text-sm font-bold truncate">CodexManager</span>
              <span className="text-xs text-muted-foreground truncate opacity-70">{t("账号池 · 用量管理")}</span>
            </div>
          )}
        </button>
      </div>

      <div className="flex-1 overflow-y-auto py-4">
        <nav className="grid gap-1 px-2">
          {renderedItems}
        </nav>
      </div>

      <div className="border-t p-2 shrink-0">
        <Button
          variant="ghost"
          size="icon"
          className="w-full justify-start gap-3 px-3 h-10"
          onClick={toggleSidebar}
        >
          {isSidebarOpen ? (
            <>
              <ChevronLeft className="h-4 w-4 shrink-0" />
              <span className="text-sm">{t("收起侧边栏")}</span>
            </>
          ) : (
            <ChevronRight className="h-4 w-4 shrink-0" />
          )}
        </Button>
      </div>
    </div>
  );
}
