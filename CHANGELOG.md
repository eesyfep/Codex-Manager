# Changelog Entry

The localized changelog files live in the language folders:

- 简体中文: `docs/zh-CN/CHANGELOG.md`
- English: `docs/en/CHANGELOG.md`
- Русский: `docs/ru/Журнал-изменений.md`
- 한국어: `docs/ko/변경-이력.md`

## 2026-05-18 模型路由 fallback 兜底修复

- 文件：`crates/service/src/model_router.rs`（`route_aggregate_candidates_for_model` 末尾兜底链路）。
- 现象：会话之前用过某 aggregate API A，A 被关闭（`status=disabled`）后，再次发起请求直接 4xx `failed_no_candidate`，没有自动 fallback 到其他 active 上游。
- 根因：路由层 binding 指向 A，A 不再 active 导致 `routed` 为空，原代码只回退到"有 capability success 的 active 候选"`fallback_model_candidates`；当 A 是该 model 唯一 success 来源时，二级回退也空，最终交出空候选导致上层耗尽。
- 修复：增加一级兜底 `fallback_candidates`（同 partition 内所有 active aggregate API），并写 `event=model_router_fallback_to_active_pool reason=bound_apis_disabled` 日志，便于排查。
- 副作用：用户 key 已设 partition 时仍受过滤限制；真无任何 active 候选时仍 4xx，符合预期。
- 同步前端 / 后端价格归一化修正（OpenRouter snapshot 中 `~xxx-latest` 别名条目过滤；`mimo-v2.5-pro` 不再被覆写成 `mimo-v2.5`；qwen-plus 不再被误归并到 qwen-max），确保 dashboard 价格估算和 request log 估价口径一致。
- 同步修复聚合 API 编辑 modal 自动搜索结果 / 绑定模型 panel 超出 UI 框问题（`apps/src/components/modals/aggregate-api-modal.tsx:837` flex column 内 scroll 容器加 `min-h-0 flex-1`）。
- 全链路验证通过：`cargo fmt --check`、`pnpm --dir apps exec tsc --noEmit -p .`、`pricing-display` 5/5、`request_log` 18/18、`gateway_logs` 20/20、Playwright `dashboard-v3-acceptance` / `logs-v3-session-name` / `aggregate-api-modal-v3` 5/5；`cargo tauri build` 重新生成 MSI / NSIS / release exe。

## 2026-05-18 公开 fork 文档整理

- 清理根 README 和多语言 README 中的原作者展示、赞助、打赏、联系方式、社区帖与 star-history 展示信息。
- 来源信息仅保留 GitHub 原仓库链接：`https://github.com/qxcnm/Codex-Manager`。
- 新增本 fork 更新摘要，集中说明模型路由、聚合 API、Codex App 模型可见性、Claude Code 兼容、请求日志、仪表盘与计费相关增强。
- 变更原因：公开 fork 需要展示当前维护版本的功能增量和日志入口，同时避免继续展示原项目个人推广信息。

## 2026-05-18 00:56 +08:00

- 收口 aggregate fast 的最后语义缺口：当平台密钥 fast / 聚合 API fast 有意图，但当前 aggregate upstream profile/path 不支持 `service_tier=priority` 注入时，request log 现在显式写入 `unsupported_by_profile`，请求日志 UI 会显示中文说明，不再把它误判成 fast 已生效。
- 重新完成 v3 定向回归：`cargo fmt --check`、`pnpm --dir apps run build:desktop`、`pnpm --dir apps exec node --test tests/pricing-display.test.mjs`、`cargo test -p codexmanager-service --test gateway_logs gateway_aggregate_api_ -- --nocapture`、`cargo test -p codexmanager-service --lib upstream_failover_policy_ -- --nocapture`、`pnpm --dir apps exec playwright test tests/dashboard-v3-acceptance.spec.ts tests/logs-v3-session-name.spec.ts tests/aggregate-api-modal-v3.spec.ts` 通过。
- 重新执行 Windows installer 打包：`cargo tauri build` 已刷新 `apps/src-tauri/target/release/bundle/nsis/CodexManager_0.2.7_x64-setup.exe` 与 `apps/src-tauri/target/release/bundle/msi/CodexManager_0.2.7_x64_en-US.msi`。
- 变更原因：只读复核确认大部分 v3 功能已在代码中存在，但 aggregate fast 仍缺少“不支持当前 profile/path”语义，且 Trellis/CHANGELOG/安装包证据需要重新与事实同步。

- 收口 Claude Code 工具 schema 观测链路：新增 `tool_schema_events_json` 存储字段与迁移 `059_request_logs_tool_schema_events.sql`，Anthropic SSE / aggregate output / response finalize 会记录脱敏后的工具 schema 校验摘要，并在请求日志 UI 展示。
- 收口第三方模型计价一致性：DeepSeek、Kimi、Qwen、Claude 等后端估算与前端价格表对齐；GLM、MiMo 和未知模型不再静默按 0 元计费，而是返回未知价格状态供 UI 明确展示。
- 完成本轮最终回归与打包：`cargo fmt --check`、`pnpm --dir apps exec tsc --noEmit`、`cargo test -p codexmanager-core --lib -- --nocapture`、`cargo test -p codexmanager-service --lib request_log -- --nocapture`、`cargo test -p codexmanager-service --lib anthropic_sse_reader -- --nocapture`、`cargo test -p codexmanager-service --lib -- --test-threads=1` 通过，其中 service lib 全量为 906 个测试通过；`cargo tauri build` 重新生成 `CodexManager_0.2.7_x64_en-US.msi` 和 `CodexManager_0.2.7_x64-setup.exe`。
- 变更原因：只读复核发现 ToolSearch 降级已有记录，但 R10.7 的工具 schema 校验事件和 R4 的第三方计价未知状态仍需要完整贯通与最终验收记录。

## 2026-05-18 00:18 +08:00

- 收口 ToolSearch 兼容日志链路：`dynamic_tools` / `dynamicTools` 降级为 upfront `tools` 时，现在会返回并贯通 `tool_search_mode=degraded_to_upfront_tools`，写入 SQLite request_logs、RPC summary，并在请求日志 UI 的“类型 / 方法 / 路径”悬浮和技术详情中展示。
- 新增迁移 `058_request_logs_tool_search_mode.sql`，并补齐普通账号轮转、Aggregate API、候选预检和 websocket/request-log 上下文里的默认传递，避免降级行为静默发生。
- 新增 ToolSearch 降级模式回归测试，并完成本轮收口验证：`cargo fmt --check`、`cargo test -p codexmanager-core --lib -- --nocapture`、`cargo test -p codexmanager-service --lib request_rewrite/request_log/anthropic/aggregate_failover/model_router/local_validation/http_bridge_tests -- --nocapture`、`pnpm --dir apps exec tsc --noEmit` 通过。
- 变更原因：新版 Trellis PRD 的 R10.4 要求 ToolSearch 不支持 `tool_reference` 时必须降级为 upfront tools，并且 request log 明确记录 `tool_search_mode=degraded_to_upfront_tools`，不能静默失败。

## 2026-05-17 23:40 +08:00

- 补强聚合 API reasoning fallback：同候选 400 unsupported reasoning 现在覆盖 OpenAI Responses `reasoning.effort`、OpenAI Chat `reasoning_effort`、Anthropic `thinking` 与 `output_config.effort`，按 `xhigh/high -> medium -> low -> remove` 降级，并在 remove 时清理 thinking/reasoning 字段。
- 补强聚合 API fast 生效语义：新增 aggregate route 层 effective fast helper，最终值为 `platform_key_fast || aggregate_api_fast`，并区分 `platform_key`、`aggregate_api`、`both`、`off` 来源日志；平台 key fast 会对非 fast 单上游注入 `service_tier=priority`。
- 变更原因：新版 Trellis PRD 要求修复 Claude Code/Anthropic/OpenAI reasoning 透传/fallback 与 aggregate fast 链路，且不得修改 core request log DTO/storage。

## 2026-05-17 05:53 +08:00

- 完成 Claude Code 调用 GPT / Claude 模型的推理透传修复，`reasoning_effort`、Anthropic `thinking` 与 `service_tier` 在 local validation、请求改写和请求日志里保持一致。
- 恢复聚合 API settings 内的 `fast` 开关，并统一平台密钥 fast 语义：平台密钥 fast 和聚合 API fast 任一开启都可生效，同时请求路径不再忽略 `candidate.fast`。
- 完成仪表盘计费合并、渠道分布切换、token/cache 趋势切换、第三方模型计价、模型路由上游可用性 hover、请求日志 Codex 项目-会话上下文，以及非 GPT 模型 400/500 failover 的回归验证。
- 验证：`pnpm --dir apps exec tsc --noEmit`、`cargo fmt --check`、`cargo test -p codexmanager-service --lib -- --test-threads=1` 通过，878 个 service lib 单元测试全部通过；`cargo tauri build` 已生成 MSI 与 NSIS 安装包。

## 2026-05-17 21:01 +08:00

- 修复 Claude Code 通过 GPT / Codex 模型返回命令工具调用时，Anthropic SSE 桥接输出空 `tool_use.input` 导致客户端出现 `undefined is not an object (evaluating 'H.command')` 的崩溃风险；命令类工具现在会保留参数并补齐 `command` 字段。
- 修复 Claude / MiMo / DeepSeek thinking 模式历史回放缺失：Anthropic `thinking` / `redacted_thinking` 历史会转换为 Responses reasoning item，Responses 转 Chat Completions 时会把 reasoning signature 回填为 assistant `reasoning_content`，thinking 模式下的 assistant tool-call 历史缺失时会补空字符串占位。
- 新增聚合 API reasoning 400 同候选降级重试：上游返回明确 reasoning / thinking 不兼容时，当前候选会按 `xhigh/high -> medium -> low -> 移除 reasoning` 继续重试，避免 MiMo 设置 high 后直接失败。
- 变更原因：用户复现 MiMo high 返回 `reasoning_content in the thinking mode must be passed back`，并确认 `H.command` 只在 GPT 模型路径突然崩溃，需要优先修复协议桥与 reasoning fallback。
- 验证：`cargo fmt --check`、`cargo test -p codexmanager-service --lib chat_completions -- --nocapture`、`cargo test -p codexmanager-service --lib anthropic -- --nocapture`、`cargo test -p codexmanager-service --lib aggregate_failover -- --nocapture` 通过；新增/相关定向测试均通过。

## 2026-05-17 01:35 +08:00

- 移除聚合 API 侧遗留 `fast` 开关的实际写入和请求体注入效果；新增/更新聚合 API 时后端强制保持 `fast=false`，前端不再展示或提交该开关，避免聚合 API 误把 `service_tier` 注入到功能 API。
- 补齐模型路由默认模型板块和 Claude/Anthropic 厂商级规则验收：默认模型板块可编辑模型组并展示 catalog、上游目录、测响和路由覆盖；Claude 厂商规则覆盖未来 Claude/Anthropic 模型，显式单模型禁用仍优先。
- 修复上游探测手动添加模型后被后续 catalog-only 刷新覆盖的问题；`success/manual` 能力不会被弱 catalog-only 结果降级，并补充回归测试。
- 补齐上游探测缓存复用策略：非强制探测会复用未过期的 `model_probe_cache` 成功/手动结果，过期结果或 API 配置更新会重新探测，失败结果在退避窗口内不重复打上游；手动测响通过 `force=true` 明确绕过缓存。
- 补齐请求日志 Anthropic thinking 与 Codex 会话信息：解析 Anthropic `thinking` 请求、`thinking_output_tokens` / `thinking_tokens` usage alias，并在日志 hover 中显示 cwd、thread、parent thread、subagent、agent、conversation、session expected、request effective 和 route source。
- 变更原因：用户复核 5 月 15 日 PRD 后指出 aggregate API fast、目标 1/3/7 和手动/自动探测合并仍未真正完成，需要按 PRD 重新补修并用定向测试验证。

## 2026-05-15 20:05 +08:00

- 发布版本提升到 `0.2.7`，同步更新 workspace、前端包、Tauri 主配置与 router-dev 配置，避免新安装包覆盖旧 `0.2.6` 产物。
- 变更原因：用户要求确认后打包 CodexManager 新版本，需要生成独立版本号的 MSI/NSIS installer。

## 2026-05-15 19:10 +08:00

- 模型路由页面接入可搜索多选规则输入，支持一次保存多个具体模型，并支持 `vendor:claude`、`vendor:anthropic`、`vendor:openai`、`family:glm/deepseek/mimo/kimi/qwen/domestic` 与 `pattern:*` 规则。
- 调整后端模型路由规则排序为显式单模型禁用、显式具体模型、厂商、模型族、pattern/all，并补充 Claude 厂商未来模型命中与显式禁用覆盖厂商的单元测试。
- 补充已确认路由设计约束：聚合 API 分区固定为 `primary/wool/anthropic/domestic`，Claude vendor 规则覆盖所有 Claude/Anthropic 且显式单模型禁用优先，400 默认进入分类 failover 并输出原始状态/错误、错误类型、上游、attempted APIs/models 和最终结果，Anthropic 兼容项覆盖 `x-api-key`、`Authorization: Bearer`、`anthropic-version`、可选 `anthropic-beta`、native Messages body 和 OpenAI chat/responses relay modes。
- 修复聚合 API `provider_partition` / `protocol_profile` 只写入 DB 但列表 RPC 按 `pool` 重新推导的问题；现在协议画像可以从 storage 到 UI/API 层真实 round-trip。
- 变更原因：任务 B 要求在不修改 storage/RPC 合同和生产 CodexManager 的前提下交付模型路由 UI 与厂商/模型族规则最小闭环。

## 2026-05-15 04:22 +08:00

- 完成 `05-12-codex-app-model-visibility-routing-state-repair` 的实现收尾：补齐 Codex App 模型可见性诊断、主会话恢复内置模型、请求事实/会话期望分层、图片 clear 回归，以及聚合 API 测活/测响拆分。
- 修复诊断语义缺口：模型可见性诊断现在区分 stored raw catalog 与 computed managed catalog，并列出所有 active platform key 的 `/v1/models` 投影，避免多 key 场景把 latest key 推断误判为真实当前 key。
- 变更原因：只读验收发现诊断入口仍可能在 fresh catalog 或多 active key 场景误导用户，需要让诊断输出直接解释第三方模型缺失位置与 key binding 过滤来源。

## 2026-05-15 12:45 +08:00

- 新增 Codex App 模型目录显式启用入口：模型路由 App 诊断页可手动校验 `model-catalog.codexmanager.json`、备份 `config.toml`，并写入唯一的顶层 `model_catalog_json`。
- 修复已导出 900+ 模型目录但 Codex App picker 仍只显示 `GPT-5.5` 的恢复路径缺口；后台模型缓存同步仍保持只导出缓存和 catalog，不再静默改写用户配置。
- 变更原因：本机复现为 `model_catalog_json` 被注释后 Codex CLI/App picker 回落到内置单模型目录，需要一个可审计、可备份、用户显式触发的修复动作。

## 2026-05-15 15:21 +08:00

- 修复 Codex App catalog 记录不完整的根因：导出的 `gpt-5.5` 等内置 slug 现在以 Codex 内置模板补全 reasoning、tool、picker 能力字段，不再被上游缓存里的 `false` 或空数组覆盖。
- 修复本机 `model-catalog.codexmanager.json` 的二次类型错误：`auto_compact_token_limit` 必须是整数，`193800.0` 会让 `codex debug models -c model_catalog_json=...` 直接解析失败。
- 变更原因：`model_catalog_json` 指向的 JSON 不是局部 patch；Codex 会按 slug 使用整条 catalog 记录，缺字段或错误数值类型都会破坏 App 模型目录。

## 2026-05-14 15:30 +08:00

- 修复 OpenAI Images 兼容入口被主会话模型 override 污染的问题；主会话恢复内置模型后，图片请求继续使用内置图片主模型和 `gpt-image-2` 工具模型。
- 修复同步 Codex 模型目录会自动激活 `model_catalog_json` 的问题；同步流程现在只导出 `models_cache.json` 和 `model-catalog.codexmanager.json`，不再改写用户 `config.toml`，避免 Codex App Desktop picker 被 custom catalog 拖成空列表。

## 2026-05-15 00:08 +08:00

- 实现聚合 API 测活 / 测响拆分：`/models` 成功不再因单个模型响应失败被判死，模型路由测响结果拆分显示 `auth`、`catalog`、`response`、`/responses`、`/chat`、`Claude Messages` 和模型来源。
- 修复 Azure OpenAI 探针选错旧 preview 模型的问题，优先使用绑定模型和配置 fallback；同时把 probe `max_output_tokens` 提升到 64，并把 SSE 探针读取窗口提升到 16KB。
- 增加国产 OpenAI-compatible / chat-only relay 目录解析与测响兼容，支持 `data/models/items` 等常见目录形状；增加 Claude / Anthropic-native Messages API 探针与 `message_stop` 终止判断。
- 变更原因：用户在 CodexManager 中测活、测响持续超时或 404，需要把 endpoint/key/catalog 可用性和具体模型真实响应能力分开判断。
