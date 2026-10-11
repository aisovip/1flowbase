# Agent Logs 原生采集器

[English](README.md)

新建日志应用默认启用“日志接收”，创建 API Key 并配置采集器后即可上传。可在应用管理或 API 页面随时停用接收。

采集器分两步安装：先从远程扩展目录**安装到 1flowbase**，再打开“下载采集 CLI”详情，在运行 Codex 的本机执行命令。安装器、原生包、校验文件和文档均从当前 1flowbase 的版本固定 URL 下载；远程仓库不可用时，已安装版本仍可下载。“平台已安装”表示分发包已留存在平台，不表示用户本机已安装或在线。

在 Agent Logs 应用的 **采集 CLI** 页面点击 Codex 页签进入详情；“全部”页签回到目录。可以粘贴当前应用的 API Key，或点击“快速生成 Key”创建并回填一个真实应用 Key。复制 Shell 或 PowerShell 安装命令，在运行 Codex 的用户电脑执行；未填写 Key 时，安装器仍会在本地终端提示输入。官方 Rust 可执行程序不要求安装 Node.js、Rust 工具链或检出源码。

从 0.3.0 起，安装器下载并校验原生发行，保存私有配置，将本应用的采集命令合并到 Codex `hooks.json`，保留其他 Hook 和 `notify`。不注册常驻采集进程或系统服务。安装后在 Codex `/hooks` 中检查并信任新增命令；安装器不绕过 Hook 信任。升级成功后移除本应用的旧采集服务，保留断点。`--no-start` 只配置采集器并取消本安装的 Hook 注册。

每个应用 ID 对应独立安装和断点。来源范围默认是安装时的 `CODEX_HOME`，未设置时使用 `~/.codex`，只允许 `sessions` 和 `archived_sessions`；支持自定义来源路径。Hook 根据 `transcript_path` 定位当前会话，子 Agent 使用 `agent_transcript_path`，不会扫描整个历史目录。首次触发会补采该会话未上传的内容；全目录历史由手动 `import` 导入，安装本身不会自动导入全部历史。源日志保持只读。

输入框只保留当前详情页内存，刷新或离开详情后清空，不写入浏览器存储或查询缓存。命令预览遮住 Key，复制命令使用真实值，通过 `FLOWBASE_AGENT_LOGS_API_KEY` 环境变量交给本地安装器；Key 不出现在下载 URL 或安装器 CLI 参数中。安装器将 Key 保存到本机私有 `config.json`。快速生成的 Key 可在应用 API 页面管理或撤销。上传仅通过 Bearer header 发送给指定 1flowbase 端点。端点是完整 `/api/logs/v1/events` URL；网络传输应使用 HTTPS，上传拒绝重定向。

[官方安装、升级与卸载说明](https://github.com/taichuy/1flowbase-official-plugins/blob/main/runtime-extensions/@taichuy/codex-logs-collector/README.md)

原生命令包括 `codex-logs-collector import --config PATH`（一次性历史导入）、`flush --config PATH`（补传待处理会话）和供 Codex 调用的 `hook --config PATH`。`watch` 保留为手动入口，安装器不会启动它。SessionStart、UserPromptSubmit、PostToolUse、Stop、SubagentStop、Interrupt 和 SessionEnd 触发短任务；多数异步执行，退出后没有监听器。Stop/Interrupt 早于结束记录落盘，采集器短暂等待，未落盘则保留待处理来源。断网或进程被终止后，由下一次 Hook 或手动 flush 恢复；没有独立的定时重试。升级保留 `state.json` 及来源身份。并发 Hook 先持久化通知，再串行获取断点锁；完整 ACK 后才移除通知。重新配置可更换 Key，改变 endpoint/source 使用独立安装。

## 恢复与来源语义

只读取以换行符结束的完整 JSONL 行，末尾半行即使暂时可解析也不提交，补全后继续。只有完整持久 ACK 才推进断点；网络失败、请求拒绝或 ACK 不完整保留旧断点。服务端提交后、断点落盘前崩溃会重放，服务端按稳定事件身份判重。已确认前缀被改写或截断时拒绝继续。从已保存的来源上下文和确认位置恢复；只对被触发的会话校验历史前缀并解析增量。

事件身份来自不可变的首行 rollout header 和字节位置，移入 `archived_sessions` 不会改变。不同位置的相同文本保留为不同事实，不凭内容猜测去重。事件时间取来源时间，不取采集主机时间。optional ordinal、`history_base` 前缀引用、parent thread、root session 均保留在 `raw`，不会与本地字节偏移或子 thread 身份混淆。

只有来源明确提供的 turn ID 才建立任务：来自 `turn_context`、task/turn-start、typed usage 或 response-item passthrough metadata。初始 session 和轮次前事实等待首个可靠 turn，随后归属该轮次，事件身份不变。没有可靠 turn 的文件会报告等待来源轮次身份，不捏造用户任务，也不推进断点。前缀引用保留为事实，不复制父 rollout 来合成子任务事件。来源已内联的继承历史按 metadata/ordinal 边界标记，后端从新任务输入和 usage 中排除继承事实。

Codex `response_item` user/assistant 提供对话事实；明确标记 `phase: "final_answer"` 的 assistant 消息声明终答。commentary、工具、compaction、上下文、未知类型及原始事实保留轨迹。`event_msg` user/agent 展示镜像保存为 context，避免重复形成对话记录。Codex `task_complete`（wire alias 为 `turn_complete`）通过非空字符串 `last_agent_message` 明确声明完成文本；adapter 将该事实映射为 `kind: "task_end", phase: "final_answer", content: <来源文本>`，保留真实 turn 归属和完整 raw。完成文本缺失、为空或仅含空白时，仍保留 `content: null, phase: null`；不生成摘要，也不从普通 assistant 顺序猜测终答。assistant-final 与 completion-final 重叠时，adapter 保留两条原始事实，由通用后端只投影一条对话终答，同时保留完整轨迹。Codex `turn_aborted` 转换为 `kind: "task_end", phase: "cancelled"`，完整保留 raw；后端消费该来源无关的取消事实。旧版 assistant 没有明确 phase 时保留未知，但明确的完成文本声明仍可提供终答；model/provider/phase 不猜测；只复制明确提供的 `model` 和 `model_provider`，不更名 provider 标识。

新 `token_usage_record.usage` 是带 `response_id` 的 response delta，`turn_token_usage/thread_token_usage` 的累计值保留 raw。旧 `event_msg.token_count.info.total_token_usage` 使用 `basis: "cumulative"`，`last_token_usage` 保留 raw。没有可靠 response ID 的历史累计快照只表示 session/thread 历史观察，不代表可归属的 task/response 消耗；它们保留轨迹和原始证据，不计入任务 token，也不猜测轮次增量。累计快照不能与 response delta 相加；只有可靠归属到 response 且该 response 没有 delta 时，其累计快照才能作为该 response 的用量。聚合、价格和计费由后端负责，CLI 不计算费用。

## API 与 adapter

向 `POST /api/logs/v1/events` 发送 `Authorization: Bearer <application-key>`。固定 `schema_version: "1flowbase.agent-logs/v1"`；精确 JSON 字段与示例见 [English API contract](README.md#api-envelope-and-receipt)。`task_end` 携带 `phase: "final_answer"` 和非空 content 时声明来源完成终答，`phase: "cancelled"` 表示取消且不含终答文本。kind 为 `system/user/assistant/tool_call/tool_result/usage/context/task_end`。usage 的 basis 为 `delta/cumulative`，可含 `response_id/input_tokens/output_tokens/input_cache_hit_tokens/cache_write_tokens/total_tokens`。

成功 HTTP body 沿用 `ApiSuccess<AgentLogsReceipt>`：`{"data":{"accepted_events":1,"duplicate_events":0,"record_ids":["00000000-0000-0000-0000-000000000001"]},"meta":null}`。CLI 只读取 `JSON.data` 中的 receipt，不接受顶层未包装字段。data 内 receipt 含 `accepted_events`、`duplicate_events`、UUID 字符串数组 `record_ids`；接受数与重复数之和必须等于完整批次事件数。服务端整批持久提交或整批失败；相同身份不同 payload 为冲突。应用 key 授权日志写入，不授权模型生成。

官方共享 Rust `agent-logs-collector` SDK 负责来源扫描、事件稳定身份、HTTP 上传、完整 ACK、断点排他与持久化。Codex 插件只实现来源适配接口和格式转换，直接引用主仓锁定 revision 的 canonical Rust DTO，不复制协议定义。

旧仓库 Node 采集器保留为开发 fixture oracle，不是用户安装入口。原生来源与恢复测试在官方插件仓库执行 `cargo test --locked --manifest-path sdk/agent-logs-collector/Cargo.toml` 和 `cargo test --locked --manifest-path runtime-extensions/@taichuy/codex-logs-collector/Cargo.toml`；安装 fixture 使用合成日志和本地 mock HTTP 端点。


## 通过 MCP 获取当前轮次身份

将 `get_current_run_context` 接口绑定为只读 MCP Tool。原生接口为 `GET /api/console/applications/logs/current-context`，无须传参。接口从当前 MCP `tools/call.params._meta` 读取客户端标识，包括 Codex 的 `threadId`、`sessionId` 和 `x-codex-turn-metadata.turn_id`；AI 不需要事先知道这些 ID。

响应包含 `thread_id`、`turn_id`、`session_id`、`application_id`、`record_id`、`native_run_id`、`log_conversation_id` 和 `status`。`record_id` 是当前轮次的日志记录 ID；存在原生运行时，`native_run_id` 是对应运行详情的锚点 ID。客户端对话、进程会话、日志对话和运行记录分别具有独立身份。

后端只查询当前工作区内、调用者对 `get_current_run_context` 操作有读取权限的应用。原生日志按客户端对话和轮次匹配；Codex 采集日志按来源对话和任务匹配。不通过最近日志、API Key 或 MCP 传输会话猜测当前运行。

| 状态 | 含义 |
| --- | --- |
| `resolved` | 唯一匹配到可读记录，返回记录定位字段 |
| `missing_identity` | 客户端未同时声明对话与轮次，或 HTTP 请求不在 MCP 调用内 |
| `invalid_identity` | 身份元数据格式不合法 |
| `conflicting_identity` | 多处身份声明不一致 |
| `not_found` | 尚无可读记录匹配；采集可能还未持久化当前轮次 |
| `ambiguous` | 多个可读记录匹配，不返回记录定位字段 |

接口仍要求认证与操作权限。客户端声明的 ID 仅用于关联，不是授权凭证。无法唯一匹配时，记录定位字段为空，不返回猜测的 ID。

## 采集费用估算

采集费用只按 `model_id` 精确匹配已有价格规则，`provider_code` 保留来源信息，不参与定价筛选。启用、事件时间和本地时窗均需有效；多个命中沿用价格列表的稳定顺序：供应商代码升序、优先级降序、生效时间降序、规则 ID 升序，取第一条。缺价格复用 `zero/any`，真实用量缺失时不补造费用。采集只记录估算，不扣余额；实际模型调用的供应商计费规则保持独立。

历史费用不会因价格配置变化自动重写。数据库维护者可显式运行 `agent_logs_reprice --application-id UUID --scope-id UUID`（通过私有环境变量 `API_DATABASE_URL` 提供数据库连接）。该命令按记录 ID 顺序从已保存的最小用量事实重算，仅更新费用；不重建消息或轨迹、不改事件身份/正文/Token，不重置断点或重复上传。每条记录原子提交，失败后可安全重跑。

## 删除日志与进度

日志页“删除日志”支持过去 7/30/90 天、一年、全部或自定义日期；选择后先统计日志记录数，批次默认 100。一条记录包含一轮对话及其完整轨迹，统计不按原始事件数计算。自定义结束日期包含当天，后端按 `started_at` 的 `[started_at_from, started_at_to)` 选择记录。

启动时后端重新确认总数，固定待删除记录 ID 和实际入库边界；预览与启动之间新增的数据可能使最终总数变化，启动后新采集的记录不加入任务。后端独立分批执行，删除与进度在同一事务提交。关闭弹窗或页面不停止删除；重新打开会查询已提交进度。服务重启后从未完成的批次恢复；停止在当前批次提交后生效，已删除数据无法恢复。网络断开只影响查询，不意味着任务失败。失败或停止保留真实已完成数量，不显示 100% 完成。

Console API 使用已登录 workspace session。每个接口独立授权；启动和停止要求 `x-csrf-token`。应用采集 Key 不能调用删除接口。

| 方法与应用内路径 | 用途 |
| --- | --- |
| `POST /api/console/applications/{id}/logs/deletion-preview` | body 为删除 scope，返回 `data.total_records` |
| `POST /api/console/applications/{id}/logs/deletion-jobs` | body 为 `{job_id, scope}`；scope 必须有正整数 `batch_size` |
| `GET /api/console/applications/{id}/logs/deletion-jobs/latest` | 返回 `data.job`，没有任务时为 `null` |
| `GET /api/console/applications/{id}/logs/deletion-jobs/{job_id}` | 查询指定任务；不属于当前应用/workspace 时返回 404 |
| `POST /api/console/applications/{id}/logs/deletion-jobs/{job_id}/stop` | 幂等请求停止，返回最新任务 |

启动的 `job_id` 由调用方生成 UUID。同一 ID 与相同 scope 重发只返回原任务，包括已结束的任务；不同 scope 返回 409。同一应用只能有一个活动删除任务；旧 `DELETE /logs` 保持可用，在活动任务期间返回 409。任务 scope 不接受调用方的 `ingested_at_before`，边界由服务端确定。

任务包含 `job_id/application_id/scope/ingested_at_before/status/total_records/deleted_records/stop_requested/error_code/created_at/updated_at`。状态为 `queued/running/succeeded/stopped/failed`；进度只来自 `deleted_records/total_records`，不要在客户端累计响应。启动响应丢失时查询原 `job_id`；必要时使用同一 ID 和原 scope 确认启动，不创建另一个任务。

## 日志接收开关

日志应用不需要发布编排版本。在应用管理或 API 页面，通过“日志接收”开关控制上传。新建日志应用默认启用接收；已有应用保持当前接收状态。停用后 `/api/logs/v1/events` 返回 HTTP 403（`agent_logs_ingestion_disabled`），不会写入新事件，已保存日志和 API Key 保留，仍可创建或撤销 Key。重新启用后，采集器会在下次 Hook 或手动 `flush` 时重试待上传内容。
