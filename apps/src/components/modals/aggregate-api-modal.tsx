"use client";

import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Clipboard, Database, ShieldCheck } from "lucide-react";
import { toast } from "sonner";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button, buttonVariants } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { accountClient } from "@/lib/api/account-client";
import { copyTextToClipboard } from "@/lib/utils/clipboard";
import { useAppStore } from "@/lib/store/useAppStore";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useI18n } from "@/lib/i18n/provider";
import { AggregateApi } from "@/types";

const AGGREGATE_API_PROVIDER_LABELS: Record<string, string> = {
  codex: "Codex",
  azure_openai: "Azure OpenAI",
  claude: "Claude",
};

const AGGREGATE_API_URL_PLACEHOLDERS: Record<string, string> = {
  codex: "例如：https://api.openai.com/v1",
  azure_openai: "例如：https://<resource>.openai.azure.com",
  claude: "例如：https://api.anthropic.com/v1",
};

const AZURE_OPENAI_ACTION_EXAMPLE =
  "/openai/deployments/<deployment>/chat/completions?api-version=2024-10-21";

interface AggregateApiModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  aggregateApi?: AggregateApi | null;
  templateApi?: AggregateApi | null;
  defaultSort?: number;
  defaultPool?: "primary" | "wool";
  defaultSupplierName?: string;
}

/**
 * 函数 `AggregateApiModal`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - params: 参数 params
 *
 * # 返回
 * 返回函数执行结果
 */
export function AggregateApiModal({
  open,
  onOpenChange,
  aggregateApi,
  templateApi,
  defaultSort = 0,
  defaultPool = "primary",
  defaultSupplierName,
}: AggregateApiModalProps) {
  const { t } = useI18n();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const [providerType, setProviderType] = useState("codex");
  const [pool, setPool] = useState<"primary" | "wool">("primary");
  const [fast, setFast] = useState(false);
  const [compatibilityMode, setCompatibilityMode] = useState(false);
  const [supplierName, setSupplierName] = useState("");
  const [sortDraft, setSortDraft] = useState("0");
  const [url, setUrl] = useState("");
  const [authType, setAuthType] = useState<"apikey" | "userpass">("apikey");
  const [authCustomEnabled, setAuthCustomEnabled] = useState(false);
  const [apiKeyLocation, setApiKeyLocation] = useState<"header" | "query">(
    "header"
  );
  const [apiKeyName, setApiKeyName] = useState("authorization");
  const [apiKeyHeaderValueFormat, setApiKeyHeaderValueFormat] = useState<
    "bearer" | "raw"
  >("bearer");
  const [userpassMode, setUserpassMode] = useState<
    "basic" | "headerPair" | "queryPair"
  >("basic");
  const [userpassUsernameName, setUserpassUsernameName] = useState("username");
  const [userpassPasswordName, setUserpassPasswordName] = useState("password");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [actionCustomEnabled, setActionCustomEnabled] = useState(false);
  const [action, setAction] = useState("");
  const [key, setKey] = useState("");
  const [generatedKey, setGeneratedKey] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const queryClient = useQueryClient();
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const unavailableMessage = canAccessManagementRpc
    ? t("服务未连接，聚合 API 暂不可编辑；连接恢复后可继续操作。")
    : t("当前运行环境暂不支持聚合 API 管理。");

  useEffect(() => {
    if (!open) return;
    const sourceApi = aggregateApi || templateApi || null;
    const nextProviderType = sourceApi?.providerType || "codex";
    setProviderType(nextProviderType);
    const nextPool =
      aggregateApi?.pool === "wool" || templateApi?.pool === "wool" || defaultPool === "wool"
        ? "wool"
        : "primary";
    setPool(nextPool);
    setFast(Boolean(sourceApi?.fast));
    setCompatibilityMode(Boolean(sourceApi?.compatibilityMode));
    setSupplierName(
      aggregateApi?.supplierName ||
        (defaultPool === "wool" ? defaultSupplierName || "" : ""),
    );
    setSortDraft(String(aggregateApi?.sort ?? defaultSort));
    setUrl(sourceApi?.url || "");
    const nextAuthType =
      sourceApi?.authType === "userpass" ? "userpass" : "apikey";
    setAuthType(nextAuthType);
    const authParams =
      sourceApi?.authParams && typeof sourceApi.authParams === "object"
        ? sourceApi.authParams
        : null;
    setAuthCustomEnabled(Boolean(authParams));
    if (nextAuthType === "apikey") {
      const location =
        authParams && authParams["location"] === "query" ? "query" : "header";
      setApiKeyLocation(location);
      const name =
        authParams && typeof authParams["name"] === "string"
          ? String(authParams["name"])
          : location === "query"
            ? "api_key"
            : "authorization";
      setApiKeyName(name);
      const format =
        authParams && typeof authParams["headerValueFormat"] === "string"
          ? String(authParams["headerValueFormat"]).toLowerCase()
          : "bearer";
      setApiKeyHeaderValueFormat(format === "raw" ? "raw" : "bearer");
    } else {
      const mode =
        authParams && typeof authParams["mode"] === "string"
          ? String(authParams["mode"])
          : "basic";
      setUserpassMode(
        mode === "headerPair" || mode === "queryPair" ? mode : "basic"
      );
      setUserpassUsernameName(
        authParams && typeof authParams["usernameName"] === "string"
          ? String(authParams["usernameName"])
          : "username"
      );
      setUserpassPasswordName(
        authParams && typeof authParams["passwordName"] === "string"
          ? String(authParams["passwordName"])
          : "password"
      );
    }
    const nextAction = sourceApi?.action ?? "";
    setAction(nextAction);
    setActionCustomEnabled(sourceApi?.action !== null && sourceApi?.action !== undefined);
    setKey("");
    setUsername("");
    setPassword("");
    setGeneratedKey("");
  }, [aggregateApi, defaultPool, defaultSort, defaultSupplierName, open, templateApi]);

  /**
   * 函数 `handleSave`
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
  const handleSave = async () => {
    if (!isServiceReady) {
      toast.info(
        canAccessManagementRpc
          ? t("服务未连接，暂时无法保存聚合 API")
          : t("当前运行环境暂不支持聚合 API 管理")
      );
      return;
    }
    if (!url.trim()) {
      toast.error(t("请输入聚合 API URL"));
      return;
    }
    if (!supplierName.trim()) {
      toast.error(t("请输入供应商名称"));
      return;
    }
    const rawSort = sortDraft.trim();
    if (!rawSort) {
      toast.error(t("请输入顺序值"));
      return;
    }
    const parsedSort = Number(rawSort);
    if (!Number.isFinite(parsedSort)) {
      toast.error(t("顺序必须是数字"));
      return;
    }
    if (!aggregateApi?.id && !key.trim()) {
      if (authType === "apikey") {
        toast.error(t("请输入聚合 API 密钥"));
        return;
      }
    }
    if (!aggregateApi?.id && authType === "userpass") {
      if (!username.trim() || !password.trim()) {
        toast.error(t("请输入账号密码"));
        return;
      }
    }
    if (authType === "userpass" && (username.trim() || password.trim())) {
      if (!username.trim() || !password.trim()) {
        toast.error(t("账号和密码必须同时填写"));
        return;
      }
    }
    if (aggregateApi?.id && aggregateApi.authType !== authType) {
      if (authType === "apikey" && !key.trim()) {
        toast.error(t("切换为 APIKey 认证时必须填写密钥"));
        return;
      }
      if (authType === "userpass" && (!username.trim() || !password.trim())) {
        toast.error(t("切换为账号密码认证时必须填写账号密码"));
        return;
      }
    }

    const authParams =
      authCustomEnabled && authType === "apikey"
        ? {
            location: apiKeyLocation,
            name: apiKeyName.trim(),
            headerValueFormat:
              apiKeyLocation === "header" ? apiKeyHeaderValueFormat : undefined,
          }
        : authCustomEnabled && authType === "userpass"
          ? {
              mode: userpassMode,
              usernameName:
                userpassMode === "headerPair" || userpassMode === "queryPair"
                  ? userpassUsernameName.trim()
                  : undefined,
              passwordName:
                userpassMode === "headerPair" || userpassMode === "queryPair"
                  ? userpassPasswordName.trim()
                  : undefined,
            }
          : null;
    if (authCustomEnabled) {
      if (authType === "apikey") {
        if (!apiKeyName.trim()) {
          toast.error(t("请输入认证参数名称"));
          return;
        }
      } else if (userpassMode !== "basic") {
        if (!userpassUsernameName.trim() || !userpassPasswordName.trim()) {
          toast.error(t("请输入账号密码参数名称"));
          return;
        }
      }
    }
    setIsLoading(true);
    try {
      if (aggregateApi?.id) {
        await accountClient.updateAggregateApi(aggregateApi.id, {
          providerType,
          supplierName,
          sort: parsedSort,
          url,
          key: authType === "apikey" ? key || null : null,
          authType,
          authCustomEnabled,
          authParams,
          actionCustomEnabled,
          action: actionCustomEnabled ? action.trim() : null,
          pool,
          fast,
          compatibilityMode,
          username: authType === "userpass" ? username.trim() || null : null,
          password: authType === "userpass" ? password.trim() || null : null,
        });
        toast.success(t("聚合 API 已更新"));
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] }),
          queryClient.invalidateQueries({ queryKey: ["apikeys"] }),
          queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
        ]);
        onOpenChange(false);
        return;
      }

      const result = await accountClient.createAggregateApi({
        providerType,
        supplierName,
        sort: parsedSort,
        url,
        key: authType === "apikey" ? key : null,
        authType,
        authCustomEnabled,
        authParams,
        actionCustomEnabled,
        action: actionCustomEnabled ? action.trim() : null,
        pool,
        fast,
        compatibilityMode,
        username: authType === "userpass" ? username.trim() : null,
        password: authType === "userpass" ? password.trim() : null,
      });
      setGeneratedKey(result.key);
      toast.success(t("聚合 API 已创建"));
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] }),
        queryClient.invalidateQueries({ queryKey: ["apikeys"] }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
      ]);
      onOpenChange(false);
    } catch (error: unknown) {
      toast.error(
        `${t("操作失败")}: ${error instanceof Error ? error.message : String(error)}`
      );
    } finally {
      setIsLoading(false);
    }
  };

  /**
   * 函数 `copyKey`
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
  const copyKey = async () => {
    try {
      await copyTextToClipboard(generatedKey);
      toast.success(t("密钥已复制"));
    } catch (error: unknown) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const applyAzureOpenAiTemplate = () => {
    setProviderType("azure_openai");
    setAuthType("apikey");
    setAuthCustomEnabled(true);
    setApiKeyLocation("header");
    setApiKeyName("api-key");
    setApiKeyHeaderValueFormat("raw");
    setActionCustomEnabled(true);
    if (!action.trim()) {
      setAction(AZURE_OPENAI_ACTION_EXAMPLE);
    }
    if (!url.trim()) {
      setUrl("https://<resource>.openai.azure.com");
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="glass-card w-[calc(100%-2rem)] max-w-[calc(100%-2rem)] overflow-hidden border border-border p-0 sm:max-w-[92vw] md:max-w-[640px] lg:max-w-[720px] xl:max-w-[760px]">
        <div className="flex max-h-[92vh] flex-col">
          <div className="border-b border-border px-5 pt-5 pb-3">
            <DialogHeader>
              <div className="mb-2 flex items-center gap-3">
                <div className="rounded-full bg-primary/10 p-2">
                  <Database className="h-5 w-5 text-primary" />
                </div>
                <DialogTitle>
                  {aggregateApi?.id ? t("编辑聚合 API") : t("创建聚合 API")}
                </DialogTitle>
              </div>
              <DialogDescription>
                {pool === "wool"
                  ? "配置短时效免费 API。保存后进入羊毛池，路由会优先尝试，失败或满载后回退主用池。"
                  : t("配置一个最小转发上游，保存 URL 和密钥后即可用于平台密钥轮转。")}
              </DialogDescription>
            </DialogHeader>
          </div>

          <div className="overflow-y-auto px-5 py-3">
            <div className="grid gap-4">
              {!isServiceReady ? (
                <div className="rounded-lg border border-border bg-muted/30 p-3 text-sm text-muted-foreground">
                  {unavailableMessage}
                </div>
              ) : null}

              <div className="grid gap-4 md:grid-cols-2">
                <div className="grid gap-2">
                  <Label htmlFor="aggregate-api-supplier-name">{t("供应商名称 *")}</Label>
                  <Input
                    id="aggregate-api-supplier-name"
                    placeholder={t("例如：官方中转、XX 供应商")}
                    value={supplierName}
                    disabled={!isServiceReady}
                    onChange={(event) => setSupplierName(event.target.value)}
                  />
                </div>

                <div className="grid gap-2">
                  <Label htmlFor="aggregate-api-sort">{t("顺序值")}</Label>
                  <Input
                    id="aggregate-api-sort"
                    type="number"
                    min={0}
                    step={1}
                    value={sortDraft}
                    disabled={!isServiceReady}
                    onChange={(event) => setSortDraft(event.target.value)}
                  />
                  <p className="text-[11px] leading-4 text-muted-foreground">
                    {t("值越小越靠前，用于聚合 API 轮转优先级")}
                  </p>
                </div>

                <div className="grid gap-2">
                  <Label htmlFor="aggregate-api-pool">池类型</Label>
                  <Select
                    value={pool}
                    disabled={!isServiceReady}
                    onValueChange={(value) =>
                      setPool(value === "wool" ? "wool" : "primary")
                    }
                  >
                    <SelectTrigger id="aggregate-api-pool" className="w-full">
                      <SelectValue>
                        {(value) => (String(value || "") === "wool" ? "羊毛池" : "主用池")}
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="primary">主用池</SelectItem>
                      <SelectItem value="wool">羊毛池</SelectItem>
                    </SelectContent>
                  </Select>
                  <p className="text-[11px] leading-4 text-muted-foreground">
                    羊毛池独立限流，满载或不可用时直接回退主用 API。
                  </p>
                </div>

                <div className="grid gap-3 rounded-lg border border-border bg-muted/20 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <Label htmlFor="aggregate-api-fast" className="text-sm">
                        fast
                      </Label>
                      <p className="text-[11px] leading-4 text-muted-foreground">
                        {t("开启后该聚合 API 使用 fast 服务等级。")}
                      </p>
                    </div>
                    <Switch
                      id="aggregate-api-fast"
                      checked={fast}
                      disabled={!isServiceReady}
                      onCheckedChange={(checked) => setFast(Boolean(checked))}
                    />
                  </div>
                  <div className="flex items-center justify-between gap-3 border-t border-border pt-3">
                    <div>
                      <Label htmlFor="aggregate-api-compatibility-mode" className="text-sm">
                        低质量中转兼容
                      </Label>
                      <p className="text-[11px] leading-4 text-muted-foreground">
                        针对首帧慢、终态弱的中转站启用流式抗断流保护。
                      </p>
                    </div>
                    <Switch
                      id="aggregate-api-compatibility-mode"
                      checked={compatibilityMode}
                      disabled={!isServiceReady}
                      onCheckedChange={(checked) =>
                        setCompatibilityMode(Boolean(checked))
                      }
                    />
                  </div>
                </div>

                <div className="grid gap-2">
                  <Label htmlFor="aggregate-api-provider">{t("类型")}</Label>
                  <Select
                    value={providerType}
                    disabled={!isServiceReady}
                    onValueChange={(value) => {
                      if (!value) return;
                      setProviderType(value);
                    }}
                  >
                    <SelectTrigger id="aggregate-api-provider" className="w-full">
                      <SelectValue>
                        {(value) =>
                          AGGREGATE_API_PROVIDER_LABELS[String(value || "")] ||
                          "Codex"
                        }
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="codex">Codex</SelectItem>
                      <SelectItem value="azure_openai">Azure OpenAI</SelectItem>
                      <SelectItem value="claude">Claude</SelectItem>
                    </SelectContent>
                  </Select>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="justify-start"
                    disabled={!isServiceReady}
                    onClick={applyAzureOpenAiTemplate}
                  >
                    Azure OpenAI 模板
                  </Button>
                </div>

                <div className="grid gap-2">
                  <Label htmlFor="aggregate-api-auth-type">{t("认证类型")}</Label>
                  <Select
                    value={authType}
                    disabled={!isServiceReady}
                    onValueChange={(value) => {
                      const next = value === "userpass" ? "userpass" : "apikey";
                      setAuthType(next);
                      setGeneratedKey("");
                      setKey("");
                      setUsername("");
                      setPassword("");
                    }}
                  >
                    <SelectTrigger
                      id="aggregate-api-auth-type"
                      className="w-full"
                    >
                      <SelectValue>
                        {(value) =>
                          String(value || "") === "userpass"
                            ? t("账号密码")
                            : "APIKey"
                        }
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="apikey">APIKey</SelectItem>
                      <SelectItem value="userpass">{t("账号密码")}</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>

              <div className="grid gap-2">
                <Label htmlFor="aggregate-api-url">{t("URL")}</Label>
                <Input
                  id="aggregate-api-url"
                  placeholder={
                    t(AGGREGATE_API_URL_PLACEHOLDERS[providerType] || "请输入 URL")
                  }
                  value={url}
                  disabled={!isServiceReady}
                  onChange={(event) => setUrl(event.target.value)}
                />
              </div>

              {authType === "apikey" ? (
                <div className="grid gap-2">
                  <Label htmlFor="aggregate-api-key">{t("密钥")}</Label>
                  <Input
                    id="aggregate-api-key"
                    type="password"
                    placeholder={aggregateApi?.id ? t("留空则保持原值") : t("请输入密钥")}
                    value={key}
                    disabled={!isServiceReady}
                    onChange={(event) => setKey(event.target.value)}
                  />
                </div>
              ) : (
                <div className="grid gap-4 lg:grid-cols-2">
                  <div className="grid gap-2">
                    <Label htmlFor="aggregate-api-username">{t("账号")}</Label>
                    <Input
                      id="aggregate-api-username"
                      placeholder={aggregateApi?.id ? t("留空则保持原值") : t("请输入账号")}
                      value={username}
                      disabled={!isServiceReady}
                      onChange={(event) => setUsername(event.target.value)}
                    />
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor="aggregate-api-password">{t("密码")}</Label>
                    <Input
                      id="aggregate-api-password"
                      type="password"
                      placeholder={aggregateApi?.id ? t("留空则保持原值") : t("请输入密码")}
                      value={password}
                      disabled={!isServiceReady}
                      onChange={(event) => setPassword(event.target.value)}
                    />
                  </div>
                </div>
              )}

              <div className="grid gap-4 xl:grid-cols-2">
                <div className="grid gap-3 rounded-xl border border-border bg-muted/20 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <Label className="text-sm">{t("自定义认证参数")}</Label>
                      <p className="text-[11px] text-muted-foreground">
                        {t("关闭则按默认规则注入认证（APIKey=Bearer，账号密码=Basic）。")}
                      </p>
                    </div>
                    <Switch
                      checked={authCustomEnabled}
                      disabled={!isServiceReady}
                      onCheckedChange={(checked) =>
                        setAuthCustomEnabled(Boolean(checked))
                      }
                    />
                  </div>

                  {authCustomEnabled && authType === "apikey" ? (
                    <div className="grid gap-3">
                      <div className="grid gap-3 md:grid-cols-2">
                        <div className="grid gap-2">
                          <Label className="text-xs">{t("位置")}</Label>
                          <Select
                            value={apiKeyLocation}
                            onValueChange={(value) =>
                              setApiKeyLocation(
                                value === "query" ? "query" : "header"
                              )
                            }
                            disabled={!isServiceReady}
                          >
                            <SelectTrigger className="w-full">
                              <SelectValue>
                                {(value) =>
                                  String(value || "") === "query"
                                    ? "Query"
                                    : "Header"
                                }
                              </SelectValue>
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="header">Header</SelectItem>
                              <SelectItem value="query">Query</SelectItem>
                            </SelectContent>
                          </Select>
                        </div>
                        <div className="grid gap-2">
                          <Label className="text-xs">{t("参数名")}</Label>
                          <Input
                            value={apiKeyName}
                            disabled={!isServiceReady}
                            placeholder={
                              apiKeyLocation === "query"
                                ? "api_key"
                                : "authorization"
                            }
                            onChange={(e) => setApiKeyName(e.target.value)}
                          />
                        </div>
                      </div>
                      {apiKeyLocation === "header" ? (
                        <div className="grid gap-2">
                          <Label className="text-xs">{t("Header 格式")}</Label>
                          <Select
                            value={apiKeyHeaderValueFormat}
                            onValueChange={(value) =>
                              setApiKeyHeaderValueFormat(
                                value === "raw" ? "raw" : "bearer"
                              )
                            }
                            disabled={!isServiceReady}
                          >
                            <SelectTrigger className="w-full">
                              <SelectValue>
                                {(value) =>
                                  String(value || "") === "raw"
                                    ? "Raw"
                                    : "Bearer"
                                }
                              </SelectValue>
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="bearer">Bearer</SelectItem>
                              <SelectItem value="raw">Raw</SelectItem>
                            </SelectContent>
                          </Select>
                        </div>
                      ) : null}
                    </div>
                  ) : null}

                  {authCustomEnabled && authType === "userpass" ? (
                    <div className="grid gap-3">
                      <div className="grid gap-2">
                        <Label className="text-xs">{t("发送模式")}</Label>
                        <Select
                          value={userpassMode}
                          onValueChange={(value) => {
                            const next =
                              value === "headerPair" || value === "queryPair"
                                ? value
                                : "basic";
                            setUserpassMode(next);
                          }}
                          disabled={!isServiceReady}
                        >
                          <SelectTrigger className="w-full">
                            <SelectValue>
                              {(value) => {
                                const v = String(value || "");
                                if (v === "headerPair") return t("Header 双字段");
                                if (v === "queryPair") return t("Query 双字段");
                                return t("HTTP Basic");
                              }}
                            </SelectValue>
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="basic">{t("HTTP Basic")}</SelectItem>
                            <SelectItem value="headerPair">{t("Header 双字段")}</SelectItem>
                            <SelectItem value="queryPair">{t("Query 双字段")}</SelectItem>
                          </SelectContent>
                        </Select>
                      </div>
                      {userpassMode !== "basic" ? (
                        <div className="grid gap-3 md:grid-cols-2">
                          <div className="grid gap-2">
                            <Label className="text-xs">{t("账号字段名")}</Label>
                            <Input
                              value={userpassUsernameName}
                              disabled={!isServiceReady}
                              onChange={(e) =>
                                setUserpassUsernameName(e.target.value)
                              }
                            />
                          </div>
                          <div className="grid gap-2">
                            <Label className="text-xs">{t("密码字段名")}</Label>
                            <Input
                              value={userpassPasswordName}
                              disabled={!isServiceReady}
                              onChange={(e) =>
                                setUserpassPasswordName(e.target.value)
                              }
                            />
                          </div>
                        </div>
                      ) : null}
                    </div>
                  ) : null}
                </div>

                <div className="grid gap-3 rounded-xl border border-border bg-muted/20 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <Label className="text-sm">{t("自定义 action")}</Label>
                      <p className="text-[11px] text-muted-foreground">
                        {t("开启后将用该 path 覆盖转发 action（例如 GLM 前缀路径）。")}
                      </p>
                    </div>
                    <Switch
                      checked={actionCustomEnabled}
                      disabled={!isServiceReady}
                      onCheckedChange={(checked) =>
                        setActionCustomEnabled(Boolean(checked))
                      }
                    />
                  </div>
                  {actionCustomEnabled ? (
                    <div className="grid gap-2">
                      <Label className="text-xs">{t("action path")}</Label>
                      <Input
                        value={action}
                        disabled={!isServiceReady}
                        placeholder={t("例如：/api/paas/v4/responses")}
                        onChange={(e) => setAction(e.target.value)}
                      />
                    </div>
                  ) : null}
                </div>
              </div>

              {generatedKey ? (
                <div className="space-y-2 border-t pt-2">
                  <Label className="flex items-center gap-1.5 text-xs text-primary">
                    <ShieldCheck className="h-3.5 w-3.5" /> {t("新密钥已生成")}
                  </Label>
                  <div className="flex gap-2">
                    <Input
                      value={generatedKey}
                      readOnly
                      className="bg-primary/5 font-mono text-sm"
                    />
                    <Button
                      variant="outline"
                      onClick={() => void copyKey()}
                      disabled={!generatedKey}
                    >
                      <Clipboard className="h-4 w-4" />
                    </Button>
                  </div>
                </div>
              ) : null}
            </div>
          </div>

          <div className="border-t border-border px-5 py-3">
            <DialogFooter>
              {!generatedKey ? (
                <DialogClose
                  className={buttonVariants({ variant: "ghost" })}
                  type="button"
                >
                  {t("取消")}
                </DialogClose>
              ) : null}
              {!generatedKey ? (
                <Button
                  onClick={() => void handleSave()}
                  disabled={!isServiceReady || isLoading}
                >
                  {isLoading ? t("保存中...") : t("完成")}
                </Button>
              ) : null}
            </DialogFooter>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
