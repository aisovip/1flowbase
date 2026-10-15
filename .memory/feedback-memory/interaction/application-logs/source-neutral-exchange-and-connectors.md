---
title: "日志交换统一语义与第三方连接器边界"
memory_type: feedback
feedback_category: architecture_boundary
created_at: "2026-10-15 12"
updated_at: "2026-10-15 20"
decision_policy: direct_reference
status: active
---

规则：Agent Logs 与 Agent Flow 的聊天日志应通过底座共享的来源中立语义和日志读写能力交换；不能把仅支持 agent_logs/imported 的专用导出路径当作两类应用已统一。底座提供标准协议兼容、身份授权、存储及通用 SDK/进程/网络能力；Langfuse 专属字段、API 分页、认证配置和导入导出交互应归连接器插件。统一日志事实不等于把外部历史导入为可执行 flow_run。

原因：用户在已完成 Agent Logs OTLP 交付后指出应用间交换能力断裂及第三方语义泄漏到底座。功能互通通过不证明插件职责和来源中立架构正确；不能通过再加一个 AgentFlow 专用分支扩大泄漏。

适用：后续日志架构诊断、跨 Agent Logs/Agent Flow 导入导出、Langfuse 等第三方接入。参考 SSH 的 managed_service/settings_pages/共享 SDK 机制，SSH 为私有商业插件，不向公开仓库搬用其源码或资产。用户随后明确实施及公开共享路径 `1flowbase-official-plugins/runtime-extensions/@taichuy/langfuse`，要求浏览器操作服务地址配置、导出聊天、导入指定 Agent Logs 后更新 Wiki。导出两种来源，导入只到 Agent Logs；此授权替代早期仅方案阶段，不重复请求批准。


规则补充：Langfuse 后台按实例一行使用宿主 `@1flowbase/data-table` 共享表格，交互参考 SSH；语言消费宿主 `ctx.ui.locale`，不在插件设置页自建语言切换按钮。连接器还需将按实例取地址与认证后调用 Langfuse 的薄封装注册为宿主操作接口。

原因：用户指出当前单连接卡片和独立“中文”按钮未复用成熟后台能力；只有导入导出 UI 不能替代可复用的 Langfuse 操作接口。适用于该插件多实例管理、设置页多语言及 API 注册的后续调整；SSH 仍是私有商业插件，参考宿主契约与交互，不复制商业源码或资产。

规则补充：Langfuse OpenAPI 只提供发现、获取和执行薄封装，沿用 `/api/console/managed-services/langfuse`；上游接口不可用或报错时返回真实状态和正文，让 AI 自行判断，不增加能力探测、备用端点、自动替代操作或重试。聊天分析必需的语义转换仍在专用日志交换中。

原因：用户明确接受现有受管插件命名空间，并纠正了为接口不可用增加通用包装的方向；适用于 Langfuse OpenAPI 和其他由 AI 按上游规范探索的连接器接口。
