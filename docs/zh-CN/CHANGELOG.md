# 更新日志

本文件用于记录 CodexManager 的对外可见变更，作为版本历史的唯一事实源。
格式参考 Keep a Changelog，并结合当前项目的实际维护方式做最小收敛。

## [Unreleased]

### Added
- 新增 Codex 图片生成兼容链路：支持官方 `image_generation` tool 透传，并提供 `/v1/images/generations` 与 `/v1/images/edits` 兼容入口，默认图片工具模型为 `gpt-image-2`。
- Codex CLI 首次接入引导新增 `auth.json` 配置步骤，明确平台 Key、`auth.json` 与 `config.toml` 的关系。
- 平台 Key 的“绑定模型”支持多选，并会为下游 `/v1/models` 按绑定模型暴露 OpenAI / Anthropic 兼容模型列表。
- 请求日志新增 `tool_schema_events_json` 字段：Claude Code / Anthropic SSE / aggregate 输出桥会记录脱敏后的工具 schema 校验摘要，包括工具名归一、schema 来源、校验状态、必填字段、归一化字段和阻断原因，便于排查 GPT 路径 `H.command` 类崩溃风险。
- 请求日志新增 `tool_search_mode` 字段：当 ToolSearch / `dynamic_tools` 因当前代理链不支持 `tool_reference` 而降级为 upfront tools 时，日志会记录并展示 `degraded_to_upfront_tools`。
- 模型路由 App 诊断页新增“启用模型目录”显式修复入口：会校验本机 `model-catalog.codexmanager.json`、备份 `config.toml`，并写入唯一的顶层 `model_catalog_json`，用于恢复 Codex App picker 的第三方模型列表。

### Fixed
- 修复模型路由 fallback 兜底：会话用过的 aggregate API 被关闭（`status=disabled`）后，再次请求时如果该 API 是该 model 唯一 `success` capability 来源，原路由会返回空候选导致 `failed_no_candidate` 直接报错。现新增 `fallback_candidates` 兜底层，回退到同 partition 内所有 active aggregate API，并写 `event=model_router_fallback_to_active_pool reason=bound_apis_disabled` 日志。
- 修复前后端价格归一化口径分歧：OpenRouter snapshot 中 `~xxx-latest` 别名条目不再先入价格 index（避免抢占 `claude-sonnet-4.6` 等具体版本）；`mimo-v2.5-pro` 不再被前端 normalize 误覆写成 `mimo-v2.5`；`qwen-plus-*` 不再被后端 normalize 误归并到 `qwen-max`，新增 `qwen-plus` 价格条目，与前端口径对齐。
- 修复 aggregate fast 最后一个语义误导：当平台密钥 fast / 聚合 API fast 有意图，但当前 aggregate upstream profile/path 不支持 `service_tier=priority` 注入时，请求日志现在会明确显示"不支持当前协议/路径"，不再误导成 fast 已生效。
- 修复 ToolSearch 降级静默不可见的问题：`dynamic_tools` / `dynamicTools` 会展开到 `tools`，并将降级模式贯通到 SQLite、RPC summary 和请求日志“类型 / 方法 / 路径”悬浮详情，避免 Claude Code/GPT 工具链排错时看不到兼容模式。
- 修复第三方模型计价静默为 0 的问题：DeepSeek、Kimi、Qwen、Claude 等模型的后端估算与前端价格表对齐；GLM、MiMo 和未知模型返回未知价格状态，不再把缺失价格表误显示成 0 元。
- 修复模型路由默认模型测响不是有效动作的问题：默认模型可用性板块现在提供单模型“测响”和“批量测响”按钮，触发真实 `modelRouter/probe/quickCall` RPC，并在结果中展示尝试上游、supplier、profile、URL 摘要和失败 profile。
- 修复 Claude Code 通过 GPT / Codex 模型返回命令工具调用时，Anthropic SSE 桥接输出空 `tool_use.input` 导致客户端出现 `undefined is not an object (evaluating 'H.command')` 的崩溃风险；命令类工具现在会保留参数并补齐 `command` 字段。
- 修复 Claude / MiMo / DeepSeek thinking 模式历史回放缺失：Anthropic `thinking` / `redacted_thinking` 历史会转换为 Responses reasoning item，Responses 转 Chat Completions 时会把 reasoning signature 回填为 assistant `reasoning_content`，thinking 模式下的 assistant tool-call 历史缺失时会补空字符串占位。
- 新增聚合 API reasoning 400 同候选降级重试：上游返回明确 reasoning / thinking 不兼容时，当前候选会按 `xhigh/high -> medium -> low -> 移除 reasoning` 继续重试，避免 MiMo 设置 high 后直接失败。
- 完成本轮 CodexManager 修复收尾：Claude Code 推理透传、仪表盘计费合并、渠道分布切换、token/cache 趋势、第三方模型计价、模型路由上游可用性 hover、聚合 API fast 语义恢复、请求日志 Codex 项目-会话上下文，以及非 GPT 模型 400/500 failover 都已按 PRD 回归验证。
- 恢复聚合 API settings 内的 `fast` 开关，并统一平台密钥 fast 与聚合 API fast 的生效语义：任一入口开启时都能启用 fast 请求改写，请求路径不再忽略 `candidate.fast`。
- 补齐模型路由默认模型板块：现在可编辑默认模型组，并同时显示 catalog、上游目录、测响和路由覆盖状态。
- 补齐 Anthropic / Claude 厂商级路由覆盖和显式单模型禁用回归测试，确保未来新增 Claude 模型仍能命中厂商规则，而单模型禁用始终优先。
- 修复上游探测“手动添加后被自动 catalog 刷新覆盖”的问题：`success/manual` 能力不会被 `catalog_only` 降级，并增加了对应回归测试。
- 修复上游探测缓存只写不读的问题：批量/非强制探测会复用未过期成功或手动缓存，过期缓存或 API 配置更新会重新探测，失败缓存遵守退避窗口；手动测响会带 `force=true` 绕过缓存。
- 修复请求日志 Anthropic thinking 和 Codex 会话 hover 缺口：请求侧识别 Anthropic `thinking`，usage 兼容 `thinking_output_tokens` / `thinking_tokens`，hover 里新增 parent thread、subagent、agent、session expected、request effective 和 route source。
- 修复 Codex App 自定义 `model_catalog_json` 指向 CodexManager catalog 后模型目录可能变空或缺少推理档位的问题：内置 `gpt-5.5` / `gpt-5.4` / `gpt-5.4-mini` / `gpt-5.3-codex` / `gpt-5.2` 现在以 Codex 内置模型模板补全 reasoning、tool 和 picker 能力字段，并阻止上游缓存里的 `false` 覆盖这些能力。
- 修复 `model-catalog.codexmanager.json` 中 `auto_compact_token_limit` 被写成浮点数时 Codex 直接拒绝读取的问题；该字段现在按整数输出，本机已验证 `codex debug models -c model_catalog_json=...` 可读取 912 个模型。
- 修复已导出 CodexManager 模型目录但 Codex App picker 仍只显示 `GPT-5.5` 时缺少安全恢复入口的问题；后台模型缓存同步仍只导出缓存文件，不会静默改写用户配置。
- 修复 Codex App 模型可见性诊断仍可能误导的问题：诊断现在区分 stored raw catalog 与 computed managed catalog，并列出所有 active platform key 的 `/v1/models` 投影，避免 fresh catalog 或多 key 场景下把第三方模型缺失误判为 catalog 不存在。
- 修复聚合 API `provider_partition` / `protocol_profile` round-trip 问题：列表 RPC 现在返回 DB 中保存的分区和协议画像，不再按 `pool` 二次推导，避免 UI/API 消费方看不到已探测或手动更新后的协议画像。
- 修复聚合 API“测活”和“测响”语义混用的问题：`/models` 目录读取成功会被视为 endpoint / auth / catalog 活着，单个模型 `/responses` 探针失败不再直接把 Key 判死。
- 修复 Azure OpenAI 探针可能从目录里选到旧 preview 模型并返回 404 的误判；探针现在优先使用绑定模型和配置 fallback，再退回目录模型。
- 修复国产 OpenAI-compatible / chat-only 中转站测响兼容性问题，目录解析支持 `data`、`models`、`items`、字符串数组与 `id/name/slug/model` 字段，并按 Responses 或 Chat Completions 能力分别记录。
- 修复 Claude / Anthropic-native 中转站测响发送 OpenAI-only 请求体的问题，能力探针改用 Messages API、便宜 fallback 模型和 `message_stop` 流式终止判断。
- 修复流式测响读取窗口过短导致已经返回内容却仍显示超时的问题，探针读取上限从 4096 bytes 提升到 16384 bytes，并识别 `event:` 与 `data.type` 两类终止信号。
- 修复桌面端同步 Codex 模型目录会自动激活 model_catalog_json 的问题；同步流程现在只导出 models_cache.json 和 model-catalog.codexmanager.json，不再改写用户 config.toml，避免 Codex App Desktop picker 被 custom catalog 拖成空列表。
- 修复官方返回的 Spark 专属额度未展示的问题，附加额度会按 `additional_rate_limits[].rate_limit` 继续解析并显示。
- 调整额度详情弹窗布局，附加额度较多时可按两列展示并滚动查看。
- 修复 Claude / Anthropic 兼容客户端选择 `anthropic-<模型名>` 映射名后，上游请求体仍保留映射名前缀的问题。
- 修复 OpenAI 兼容客户端使用未绑定模型的平台 Key 获取 `/v1/models` 时仍收到 Codex 私有 `models` 结构的问题，CC Desktop Switch 等客户端现在可解析标准 `data` 模型列表。
- 修复主会话模型 override 只能解锁但不能恢复内置模型的问题，模型路由页现在提供“恢复内置模型”，并同步清理派生 runtime anchor 的镜像状态。
- 修复请求日志把请求路由模型 / reasoning 与会话期望模型 / reasoning 混在一起展示的问题，日志页现在分别显示 request effective 与 session expected，并暴露 conversation binding anchor。
- 修复 Codex App 运行期刷新 `/v1/models` 后模型能力字段丢失的问题，带 `client_version` 的 Codex 模型目录请求现在继续返回完整 `models` 结构与 reasoning 元数据，避免模型菜单退化为缺少推理档位的 OpenAI `data` 列表。
- 修复 Web transport 下子 Agent 会话模型设置 / 清除缺少 RPC 映射的问题。
- 修复 Codex App 线程选择其它模型后，网关请求仍只读取旧 `session_model_memory` 并回落到 `gpt-5.5` 的问题；请求热路径现在会读取 `state_5.sqlite` 最新线程模型，并让较新的 Codex state 覆盖陈旧 `source=state` 记忆。
- 修复 `reasoning_effort=none` 被规范化丢弃，导致最终请求继续使用平台 Key 默认 reasoning 的问题。
- 修复模型路由页与平台 Key 弹窗无法显式选择 `reasoning_effort=none` 的问题，区分“跟随默认”和“显式 none”。
- 修复主会话“恢复内置模型”可能继承同 workspace 其它会话最近自定义模型的问题，clear 路径现在只使用显式 workspace 默认、全局默认或产品内置默认。
- 修复普通候选 fallback strip session affinity 时仍递归删除 nested `encrypted_content` 的问题，避免非 aggregate retry 路径丢失 thinking context。
- 修复聚合 API `/v1/responses` 透传与 stateless retry 会删除嵌套 `reasoning.encrypted_content` 的问题，避免 DeepSeek/Kimi/MiMo thinking 模型在续接轮次因缺少 reasoning context 返回 400 并被包装成 502。
- 修复 Responses 转 Chat Completions 时未把 reasoning item 的 `encrypted_content` 映射回 assistant `reasoning_content` 的问题，并在 Chat Completions 流式桥接中把上游 `reasoning_content` 回写为 Responses reasoning output item。
- 修复桌面端首页启动快照丢弃轻量参数的问题，首页不再因重统计分支超时而一直显示骨架屏或零数据。
- 修复同步 Codex 模型目录时只写 `model_catalog_json`、不校正 `[model_providers.cm].base_url` 的问题，避免 Codex App 继续请求错误端口后只能看到或调用错误模型。
- 修复普通 Codex 平台 Key 选择 GLM/Kimi/MiMo/Minimax 等已探测成功的聚合模型后仍走官方账号轮转的问题；非内置 GPT 模型现在会自动使用模型路由的聚合 API 候选。
- 修复模型目录读取会在本地刷新时仍触发远端发现的问题；远端超时会回退本地缓存，并内置常用模型目录明细，避免模型列表变空或无法新增模型。
- 修复模型管理“远端并入”在远端 10060 超时时仍弹红色失败的问题，现在会自动回读本地模型目录并保留可见列表。
- 修复“远端并入”未稳定并入号池/聚合 API 模型的真实链路问题：模型目录刷新会先合并已成功探测的 `upstream_model_capabilities`，并跳过指向 CodexManager 本机服务的自引用聚合 API，避免 `号池 -> localhost:48760/v1/models` 递归探测拖慢或误报失败。
- 修复桌面端模型目录 RPC 仍使用 10 秒默认读超时的问题；`apikey/modelCatalogList` 与 `apikey/models` 现在使用 90 秒模型目录刷新超时，避免服务端真实刷新完成前被 Tauri 客户端截断为 10060。
- 修复仪表盘历史 Token/费用总览被轻量快照降级到最近请求日志的问题，历史分布恢复使用 `request_token_stats` 全量聚合。
- 修复桌面端同步 Codex 模型目录时会自动改写用户 `config.toml` 的 P0 问题；同步流程现在只写模型缓存和 catalog 文件，不再覆盖 provider、`base_url` 或 `model_catalog_json`，并内置 GPT 系列保底模型避免 Codex App 模型菜单被空目录拖空。

### Changed
- 2026-05-18：重新执行 v3 定向回归与 Windows installer 打包。已通过 `cargo fmt --check`、`pnpm --dir apps run build:desktop`、`pnpm --dir apps exec node --test tests/pricing-display.test.mjs`、aggregate failover / failover policy 定向 Rust 测试，以及 dashboard / logs / aggregate-api 的 Playwright smoke；`cargo tauri build` 已刷新 `CodexManager_0.2.7_x64-setup.exe` 与 `CodexManager_0.2.7_x64_en-US.msi`。
- 2026-05-15：模型路由 App 诊断页新增 computed/stored catalog 对照与 active key projection 表，明确“推断 key”和真实多 key 过滤面的区别。
- 2026-05-15：模型路由测响结果拆分显示 `auth`、`catalog`、`response`、`/responses`、`/chat`、`Claude Messages` 与模型来源，便于区分“Key / 目录可用”和“某个模型真实响应失败”。
- 2026-05-13：首页默认使用轻量启动快照，保留账号、今日用量和最近请求日志，Token/模型趋势由最近日志回退生成，降低大日志库下的启动阻塞风险。
- 2026-05-13：模型路由页新增“App 诊断”入口，同时展示 raw catalog、App-visible projection、当前 platform key `/v1/models` 投影、绑定模型和目标模型命中状态，用于定位 Codex App 只显示单模型的实际过滤层。
- 2026-05-13：会话模型状态增加“状态库较新”标识，避免 Codex App 当前线程状态被本地自动记忆误判为自定义覆盖。
- 发布版本提升到 `0.2.6`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。
- 发布版本提升到 `0.2.7`，同步更新 workspace、前端包、Tauri 桌面端与 router-dev 元数据，避免新打包覆盖旧 `0.2.6` 安装包。
- README 不再展示最近提交块，首页只保留稳定的功能与文档入口。
- 设置页恢复“上游总超时”入口，`CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS` 可通过网关传输设置直接查看和修改，默认 `0` 表示不按总时长截断。
- Nginx 示例配置新增 `/v1/images/` 专用代理块，覆盖图片上传、大体积 `b64_json` 响应与长耗时图片生成场景。
- 请求日志费用估算同步官方 `gpt-5.5`、`gpt-5.5-pro` 与 `gpt-image-*` 价格；图片模型因当前 usage 无 modality 分桶，按官方 Image token 单价保守估算。

## [0.2.3] - 2026-04-15

### Fixed
- 修复原生 Codex 在线程续接场景下的锚点优先级回归：当请求已携带 `conversation_id` 或 `x-codex-turn-state` 时，不再让请求体里的 `prompt_cache_key` 抢占主导权，减少兼容模式下 resume / 上下文续接异常。
- 补齐原生锚点、显式 `prompt_cache_key`、冲突锚点与 Anthropic 原生链路的专项回归测试，避免后续再次把兼容层字段提升到高于原生 Codex 语义的优先级。

### Changed
- 发布版本提升到 `0.2.3`，同步更新 workspace、前端包、Tauri 桌面端与版本锁文件。

## [0.2.0] - 2026-04-12

### Added
- 主页面导航新增后台 keep-alive 缓存与更显眼的整区加载遮罩，桌面端、Web 版与 Docker 部署在页面切换和空闲后回访时的体感更稳定。
- 请求链路排查补齐针对 `service_tier`、Cloudflare challenge 与兼容转发的专项回归测试，覆盖原生 Codex、Claude Code 与 Gemini CLI 关键路径。

### Fixed
- 收敛 Codex 原生直通路径，默认保持官方请求形状，只保留账号选择、认证替换、路由、会话亲和与必要的内部字段处理；Claude / Gemini 继续走协议适配。
- 对齐官方 Codex 出站行为：发往 `chatgpt.com/backend-api/codex/responses` 的 `service_tier=fast` 现会正确映射为上游 `priority`，`/responses/compact` 不再错误携带 `service_tier`。
- 修复 Claude 兼容链路里 `fast` 服务等级映射与错误流模型回显不稳定的问题，减少 `403` 排查时的误导信息。

### Changed
- 发布版本提升到 `0.2.0`，同步更新 workspace、前端包、Tauri 桌面端与对外版本说明。

## [0.1.19] - 2026-04-08

### Added
- 聚合 API 现已支持多认证类型与自定义 `action` 配置，透传路由会按认证方式与动作正确命中对应上游。
- 国际化新增语言持久化与拆分后的消息目录，补齐了仪表盘、模态窗、侧边栏和用量标签等剩余多语言文案。

### Fixed
- 修复聚合 API 透传流里对 Anthropic `message_stop` 事件的识别问题，减少流式响应提前终止或状态误判。
- 修复网关继续把不受支持的 `service_tier` 发往上游的问题；标准 Responses 请求现在只保留受支持值，避免上游拒绝。
- 调整协作与安全入口页为默认中文正文，并整理多语言文档入口，减少发版后从根文档跳转时的断层感。

### Changed
- 正式文档已按语言目录重组，根入口文档与多语言首页文案同步整理。
- 发布版本提升到 `0.1.19`，同步更新 workspace、前端包、Tauri 桌面端、锁文件、README 与 CHANGELOG 的版本说明。

## [0.1.18] - 2026-04-06

### Added
- 新增账号列表直接排序控制与“低额度优先”排序，排查额度紧张账号时更直接。
- 新增 Gemini CLI 基础兼容，补齐 `tools` 流式、MCP 工具名、SSE `tool_call` 等关键链路。

### Fixed
- 修复账号页“额度详情”悬浮卡位置偏移问题，右侧浮层改为按额度概览卡片中线对齐。
- 修复 Gemini completed 工具结果误当正文、流式缓存词元日志采集、请求适配兼容与 token 刷新边界等问题。

### Changed
- 对齐 Gemini → Codex / Responses 请求链路到 CPA 兼容方向，请求会补齐 developer message、tool name 映射、FIFO `call_id`、`reasoning`、`include`、`parallel_tool_calls` 等字段。
- 清理 Gemini 路线未使用代码，并补充 CPA 鸣谢与版本文档说明。
- 发布版本提升到 `0.1.18`，同步更新 workspace、前端包、Tauri 桌面端、锁文件、README 与 CHANGELOG 的版本说明。

## [0.1.17] - 2026-04-05

### Added
- 请求日志新增“最终生效服务等级”口径，HTTP / WS 日志现在会同时保留客户端显式 `service_tier` 和请求改写后的最终值，方便核对平台 Key 默认 `Fast` 是否真正发往上游。
- 设置页新增全局“模型转发规则”，支持使用 `pattern=target` 形式做模型名改写，并在运行时请求改写阶段生效。

### Changed
- 普通平台 Key 的协议类型收敛为“通配兼容 (Codex / Claude Code)”，默认按请求路径自动选择 Claude 或 Codex / OpenAI 语义，减少重复维护多套 Key 的成本。
- 发布版本提升到 `0.1.17`，同步更新 workspace、前端包、Tauri 桌面端、锁文件、README 与 CHANGELOG 的版本说明。

## [0.1.16] - 2026-04-05

### Added
- 新增 `/v1/responses` WebSocket 请求支持，补齐传输类型识别、请求头归一化、代理运行时与请求日志链路。
- 账号页与用量弹窗新增附加额度窗口展示；刷新后会统一显示标准额度与 Code Review / Spark 等额外额度的剩余额度和重置时间。
- 网关 trace 新增 `CLIENT_SERVICE_TIER` 事件，记录 HTTP / WS 原始请求是否显式携带 `service_tier`、原始值以及日志归一化值，便于快速区分客户端显式 `fast` 与平台 Key 默认服务等级。

### Fixed
- 修复 HTTP 与 WS 请求日志中 `service_tier` 口径不一致的问题；现在仅当客户端请求自己显式携带 `service_tier` 时才记录 `fast`，不再把平台 Key 默认值误记成请求显式开启。
- 修复日志页服务等级展示与网关 on-wire 值不一致的问题；`priority` 会统一展示为 `fast`，未显式携带服务等级的请求继续显示为 `auto`。

### Changed
- 发布版本提升到 `0.1.16`，同步更新 workspace、前端包、Tauri 桌面端、锁文件、README 与 CHANGELOG 的版本说明。

## [0.1.15] - 2026-04-03

### Changed
- 发布版本提升到 `0.1.15`，同步更新 workspace、前端包、Tauri 桌面端、运行文档与 README 的版本说明。

## [0.1.14] - 2026-03-30

### Added
- 设置页新增“系统推导”按钮和“单账号并发上限”，可以按当前机器资源一键回填并立即生效。
- 入口层新增短队列等待与超载快速退化，避免高并发直接拖死服务进程。

### Changed
- README、workspace、前端包、Tauri 桌面端与版本一致性校验脚本统一提升到 `0.1.14`。

## [0.1.13] - 2026-03-25

### Added
- 新增“聚合 API”管理页，支持供应商名称、顺序优先级、按 `Codex / Claude` 分类、连通性测试与最小转发上游管理。
- 平台密钥新增 `账号轮转 / 聚合 API 轮转` 策略，聚合 API 轮转会按顺序优先命中对应供应商，再继续下一个渠道。

### Fixed
- 修复桌面端服务启动与页面切换时的自动恢复行为，避免关停后被切页重新拉起，也避免断连时仪表盘误清空数据。

### Changed
- README、workspace、前端包、Tauri 桌面端与版本一致性校验脚本统一提升到 `0.1.13`。

## [0.1.12] - 2026-03-20

### Fixed
- 修复平台密钥名称编辑链路在桌面端未完整透传的问题；现在 Web 与桌面端都能正确保存并回显名称，且支持中文名称。
- 修复平台密钥列表中密钥 ID 默认被截断的问题；现在会直接完整显示，便于核对与排查。

### Changed
- README 移除赞助、收款和联系方式主入口，改为突出当前 fork 更新、来源链接和文档导航。
- 发布版本提升到 `0.1.12`，同步更新 workspace、前端包、Tauri 桌面端、版本一致性校验脚本与 README 最新版本说明。

## [0.1.11] - 2026-03-20

### Added
- 账号管理新增封禁识别、封禁筛选与“一键清理封禁账号”入口；`account_deactivated` 与 `workspace_deactivated` 会被自动识别为不可用信号，并可在列表中直接筛选和清理。
- 账号列表的 5 小时 / 7 天额度列现在会展示各自窗口的重置时间；仅返回 7 天窗口的 free 账号也会把重置时间显示到 7 天列。
- 平台密钥新增服务等级配置：`跟随请求`、`Fast`、`Flex`，其中 `Fast` 会映射为上游 `priority`，`Flex` 会直传为 `flex`。

### Fixed
- 修复桌面端平台密钥创建 / 编辑时 `serviceTier` 未透传导致“服务等级”保存后不生效、不回显的问题。
- 修复 Web 端在非首页刷新时偶发下载错误文件的问题，并修复部分运行环境下复制 API Key / 登录链接时 `navigator.clipboard.writeText` 不可用导致的复制失败。
- 修复设置页“检查更新”按钮在自动静默检查更新时持续错误转圈的问题；现在只有手动点击时才显示加载状态。

### Changed
- 网关主链路继续向 Codex-first 收口：会话绑定、自动切号即切线程、`originator` / `User-Agent` / 请求压缩等出站语义已进一步对齐，并移除了旧兼容路径遗留的 upstream cookie 链路。
- 设置页补回服务监听地址切换，可在 `localhost` 与 `0.0.0.0` 之间切换；README 与文档也已同步收口到当前主线路径。
- 发布版本提升到 `0.1.11`，同步更新 workspace、前端包、Tauri 桌面端、版本一致性校验脚本与 README 最新版本说明。

## [0.1.10] - 2026-03-18

### Fixed
- 修复 Web / Docker 版误走桌面专属命令分支、账号启用 / 禁用缺少 `sort` 参数导致无法切换状态，以及账号详情刷新失败后状态列不及时回刷的问题。
- 修复禁用账号仍参与手动批量刷新与后台用量轮询的问题；批量刷新与后台轮询现已跳过手动禁用账号，并按并发 worker 执行。
- 修复账号状态语义混乱问题：手动禁用统一为 `disabled`，额度用尽与 `usage endpoint 401` 统一为 `unavailable`，`refresh token 401` 相关链路也统一落成 `unavailable`，前端状态展示同步收口为“已禁用 / 不可用”。
- 修复 Windows 本地 Web 启动器关闭控制台窗口后 `codexmanager-service` / `codexmanager-web` 仍残留后台的问题；启动器现在会通过 Job Object 一并回收子进程。

### Changed
- 发布版本提升到 `0.1.10`，同步更新 workspace、Tauri 桌面端、前端包版本、README 最新版本说明和版本一致性测试。

## [0.1.9] - 2026-03-18

### Added
- 请求日志现在支持后端分页、后端统计、首尝试账号和尝试链路展示，便于区分实际命中账号与 failover 后的最终账号。
- 设置页新增 free / 7 天单窗口账号使用模型配置，free 类账号会统一按设置模型发起请求。

### Fixed
- 修复桌面端启动误判、`/rpc` 空响应、`spawn_blocking` 缺失导致的刷新失败、用量弹窗刷新不同步、首次切页卡顿、Hydration 不一致等稳定性问题。
- 修复 refresh token 误摘号、free 账号请求模型未正确改写、优先账号行为不稳定，以及 `503 no available account` 缺少上下文诊断的问题。
- 修复 release workflow 中 pnpm 版本与当前锁文件不匹配导致的 verify 失败问题。

### Changed
- 旧前端已移除，桌面端与 Web 管理界面统一收口到新的 `apps` 前端；账号管理、平台密钥、请求日志、设置页和导航布局都做了整轮桌面优先重构。
- Codex 请求链路继续按实际 on-wire 行为收口：登录 / callback / workspace 校验、refresh 语义、`/v1/responses` 与 `/v1/responses/compact` 重写、线程锚点、请求压缩、错误摘要和 fallback 诊断均已继续对齐。
- 网关失败诊断和磁盘日志继续收敛，compact 假成功体、HTML/challenge 页、`401 refresh` 子类和 exhausted 候选链路都会输出更明确的摘要。
- 统一将发版版本提升到 `0.1.9`，同步更新 workspace、Tauri 桌面端、`tauri.conf.json` 与前端包版本。
- GitHub Release workflow 中固定的 Tauri CLI 版本已对齐到当前 Rust 侧实际使用版本，减少打包阶段的 CLI / crate 漂移风险。
- 发布文档与 README 已同步更新到 `v0.1.9`，并修正前端静态导出目录说明为 `apps/out`。

## [0.1.8] - 2026-03-11

### Fixed
- Removed the default `https://api.openai.com/v1` fallback path for ChatGPT-backed requests; upstream `challenge` and `403` outcomes are now returned from the primary login-account path instead of being rewritten into local fallback errors.
- ChatGPT login-account requests now recover from `401` by refreshing the local `access_token` with the stored `refresh_token` and retrying the current request once.

### Changed
- ChatGPT login-account turns now use `access_token` directly on the primary upstream path and no longer mix in `api_key_access_token` semantics.
- Synthetic gateway terminal failures now return structured OpenAI-style `error.message / error.type / error.code` payloads while keeping the existing trace and error-code headers.

## [0.1.7] - 2026-03-11

### Added
- 设置页新增网关传输参数：支持直接配置上游流式超时与 SSE keepalive 间隔，并在 service 运行时热生效。
- 桌面端启动快照补齐：仪表盘统计、账号用量状态、请求日志首屏会优先恢复最近一次快照，减少源码运行或服务重启后的全 0 / 未知状态。

### Fixed
- 修复 `codexmanager-web` 的访问密码会话跨重启仍可继续使用的问题；关闭并重新打开 Web 进程后，旧登录 Cookie 会失效，需要重新验证密码。
- 修复源码运行 `codexmanager-web` 时的启动与根路由兼容问题，减少 Web 静态资源与根路径在 Axum 路由下的不一致行为。
- 修复长输出场景下的 SSE 空闲断流重连问题，降低长时流式响应被误判中断的概率。
- 修复设置页保存上游代理、平台密钥创建弹窗关闭与重复提交、登录成功后账号表格未刷新等桌面交互问题。
- 修复模型拉取默认附加版本参数导致的部分上游兼容性问题，模型请求改为默认不附带版本号。
- 修复账号导入与登录回调两条链路的账号归并逻辑不一致问题，统一按同一身份规则新增或更新账号。
- 修复 Claude / Anthropic `/v1/messages` 适配在多 MCP server 场景下的工具截断问题；不再因前 16 个工具占满而丢失后续 server 的工具。
- 修复 Claude / Anthropic `/v1/messages` 链路缺少长工具名缩短与响应还原的问题，避免 MCP 工具名过长时映射不稳定。

### Changed
- 网关失败响应增加结构化 `errorCode` / `errorDetail` 字段，并同步补充 `X-CodexManager-Error-Code`、`X-CodexManager-Trace-Id` 响应头，便于客户端与日志系统追踪失败链路。
- 协议适配继续对齐 Codex / OpenAI 兼容生态：进一步统一 `/v1/chat/completions`、`/v1/responses`、Claude `/v1/messages` 的转发语义，并稳固 `tools` / `tool_calls`、thinking / reasoning、流式桥接和响应还原链路。
- 设置页与运行时配置继续收敛：背景任务、网关传输、上游代理、Web 安全等高频配置统一由 `app_settings` 持久化并回填到当前进程。
- 桌面与 service 启动链路继续治理，收敛 Web / service / desktop 之间的启动边界与启动顺序，减少源码运行与打包运行的行为分叉。
- 项目内部继续推进长期维护向的重构治理：前端主入口、设置页、请求日志视图、Tauri 命令注册、service 生命周期、gateway protocol adapter、HTTP bridge、upstream attempt flow 等区域已进一步拆分模块边界，减少大文件与根层门面耦合。
- service / gateway 目录结构继续收敛，更多通配导入、跨层直连和超长门面清单已被显式依赖与分层模块替代，后续维护和协议回归定位成本更低。
- 发布链路继续收敛到 `release-all.yml` 单入口，并复用前端构建产物与协议回归基线，减少重复构建与发布时的协议回归风险。

## [0.1.6] - 2026-03-07

### Fixed
- 修复 `release-all.yml` 在手动关闭 `run_verify` 时仍强依赖预构建前端工件的问题；各平台任务缺少 `codexmanager-frontend-dist` 时会自动回退到本地 `pnpm install + build`。

### Changed
- Windows 桌面端发布产物继续收敛，仅保留 `CodexManager-portable.exe` 便携版，不再额外生成 `CodexManager-windows-portable.zip`。
- 完善 SOCKS5 上游代理支持与归一化，并补充设置页中的代理协议提示文案。

## [0.1.5] - 2026-03-06

### Added
- 新增“按文件夹导入”：桌面端可直接选择目录，递归扫描其中 `.json` 文件并批量导入账号。
- 新增 OpenAI 上游代理配置与请求头收敛策略开关，可在设置页直接保存并即时生效。
- 补充 chat tools 命中探针脚本，便于本地验证工具调用是否真正命中与透传。

### Fixed
- 修复 `tool_calls` / `tools` 相关回归：补齐 chat 聚合路径中的工具调用保留、工具名缩短与响应还原链路，避免工具调用在 OpenAI 兼容返回、流式增量和适配转换中丢失或名称错乱。
- 完善 OpenClaw / Anthropic 兼容返回适配，确保工具调用、SSE 增量和非流式 JSON 响应都能按兼容格式正确还原。
- 请求日志追踪增强，补充原始路径、适配路径和更多上下文，便于定位 `/v1/chat/completions -> /v1/responses` 转发与协议适配问题。

### Changed
- 网关协议适配进一步对齐 Codex CLI：`/v1/chat/completions` 与 `/v1/responses` 两条链路统一收敛到 Codex `responses` 语义，上游流式/非流式行为与官方更接近，兼容 Cherry Studio 等客户端的 OpenAI 兼容调用。
- 设置页顶部常用配置改为统一的三列行布局，代理配置与其保持一致；同时支持关闭窗口后隐藏到系统托盘运行。
- 发布流程整合为单一一键多平台 workflow，并收敛桌面端产物形态；Windows 直接提供 portable exe，macOS 统一使用 DMG 分发。

## [0.1.4] - 2026-03-03

### Added
- 新增“一键移除不可用 Free 账号”：批量清理“不可用 + free 计划”账号，并返回扫描/跳过/删除统计。
- 新增“导出用户”：支持选择本地目录并按“一个账号一个 JSON 文件”导出。
- 导入兼容增强：支持 `tokens.*`、顶层 `*_token`、camelCase 字段（如 `accessToken` / `idToken` / `refreshToken`）自动识别。

### Fixed
- 兼容旧 service：前端导入前会自动归一化顶层 token 格式，避免旧版后端报 `missing field: tokens`。

### Changed
- 账号管理页操作区整合为单一“账号操作”下拉菜单，替代右侧多按钮堆叠，界面更简洁。

[Unreleased]: https://github.com/qxcnm/Codex-Manager/compare/v0.2.6...HEAD
[0.2.6]: https://github.com/qxcnm/Codex-Manager/compare/v0.2.3...v0.2.6
[0.2.3]: https://github.com/qxcnm/Codex-Manager/compare/v0.2.0...v0.2.3
[0.2.0]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.2.0
[0.1.19]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.19
[0.1.17]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.17
[0.1.16]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.16
[0.1.15]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.15
[0.1.14]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.14
[0.1.13]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.13
[0.1.12]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.12
[0.1.11]: https://github.com/qxcnm/Codex-Manager/compare/v0.1.10...v0.1.11
[0.1.10]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.10
[0.1.9]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.9
[0.1.8]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.8
[0.1.7]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.7
[0.1.6]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.6
[0.1.5]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.5
[0.1.4]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.4
