# 使用 OTLP 与 Langfuse 交换 Agent Logs

[English](otlp-exchange.md)


可以将多条日志导出为 OTLP JSON，在兼容系统间交换；也可以把多条日志发送到 Langfuse 项目，或把多个 Langfuse Trace 导入指定 1flowbase 应用。现有原生采集器的安装、事件上传和恢复行为保持独立。

## 准备应用与凭据

创建 **Agent Logs** 应用；需要 SDK 上报时，创建该应用的 API Key，并启用「日志接收」。Console 交换需要登录正确工作区，并拥有对应操作权限。应用 Key 授权 Trace 上报，不授权 Console 交换。

Langfuse 需要项目的 **Public Key**、**Secret Key** 和部署根地址，例如 `https://YOUR_LANGFUSE_HOST`。填写部署地址，不填写 Trace 页面或 `/api/public/otel/v1/traces`。1flowbase 服务端必须能访问该部署。交换窗口只在当前内存中保留凭据，关闭后清空；Secret Key 遮罩显示。

日志可能包含提示词、代码和工具输出，请选择实际需要共享的记录与目标。

## 选择完整会话，导出 OTLP JSON

1. 进入应用的「日志」页面，勾选需要交换的会话。每条会话可以包含多个任务或聊天轮次；导出包含所选会话的全部 imported 来源任务事实。
2. 点击「日志交换」，选择「导出 OTLP JSON」，再点击「执行交换」。
3. 保存下载的 JSON 文件。它是标准 OTLP Trace 数据，不是平台备份或原生采集器归档。

在全局日志页面，跨应用选择会按应用分别导出。导入没有隐含的跨应用目标，必须明确选择目标应用。

![选择多条日志并打开日志交换](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/01-record-selection.png)

文件导入时，在「日志交换」选择「导入 OTLP JSON」，选择内容为 OTLP JSON 对象的文件，然后执行。核对服务端回执：`accepted_events`、`duplicate_events` 是事件数，`record_ids` 是生成的日志记录 ID。事件数不等于对话数。

## 导出多条记录到 Langfuse

1. 勾选记录，打开「日志交换」，选择「导出到 Langfuse」。
2. 填写目标项目的「Langfuse 地址」、Public Key 和 Secret Key。
3. 执行后核对 `exported_records`、`exported_spans`、`trace_ids`。`exported_records` 是解析后的任务记录数，不是页面勾选的会话行数；一条会话可以导出多个任务。
4. 在 Langfuse 打开同一项目，根据回执中的 Trace ID 定位，逐项查看输入、输出、Span 和 metadata 是否与源记录对应。

![填写多记录 Langfuse 导出配置](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/02-langfuse-export.png)

![在目标 Langfuse 项目定位导出的 Trace](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/03-langfuse-traces.png)

服务端使用 Public Key 和 Secret Key 组成 HTTP Basic 认证，向 Langfuse 的 `/api/public/otel/v1/traces` 发送 OTLP Trace。这些是 Langfuse 项目凭据，不是 1flowbase 应用 Key。

Trace 身份对应 source/session 组合；task 和 event 映射为该 Trace 内的 Span 层级。因此同一 session 的多条任务记录可以共享 Trace ID。不能将 `exported_records` 与会话行数或 `trace_ids` 数量等同。教程演示数据包含两个会话、三个任务、21 个事件，对应三个任务记录、两个 Trace、21 个 Span。

## 从 Langfuse 导入多个 Trace

1. 进入目标应用「日志」页面；在全局日志页面时明确选择导入目标应用。
2. 在「日志交换」选择「从 Langfuse 导入」，填写部署地址和项目凭据。
3. 粘贴多个 Trace ID，用逗号或换行分隔，执行交换。
4. 核对 `accepted_events`、`duplicate_events`、`record_ids`，打开对应记录查看导入的对话与轨迹。同一身份且内容不变的重传计为重复；同一身份下内容冲突会报错。

![指定目标并填写多个 Langfuse Trace ID](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/04-langfuse-import.png)

![查看导入记录的对话](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/05-imported-conversation.png)

远程导入通过 Langfuse public observations-v2 API 读取指定 Trace ID 的 observation 数据，包含全部 observation 分页。只有明确的能力不存在响应（404）才使用 legacy observations API，不要求另一个 traces API 提供数据。认证、网络或服务端错误需直接处理，不能解释为旧版 API 信号。

v2 请求使用 `expandMetadata=1flowbase_agent_log` 获取完整的自身导出 metadata，避免默认截断表达。响应或嵌入的 source-event envelope 格式错误时拒绝导入，不会静默当作恢复成功。

<details>
<summary>移动端截图</summary>

![移动端选择会话](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/mobile-01-record-selection.png)

![移动端导出到 Langfuse](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/mobile-02-langfuse-export.png)

![移动端 Langfuse Trace 页面](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/mobile-03-langfuse-traces.png)

![移动端导入 Trace](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/mobile-04-langfuse-import.png)

![移动端查看导入会话](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/mobile-05-imported-conversation.png)

</details>

## 使用 OpenLIT SDK 直接上报 Trace

接收端只支持 **HTTP Trace**：`POST /api/logs/otlp/v1/traces`，JSON 或 protobuf，可使用 gzip。本轮不提供 gRPC、Metrics 或 Logs receiver。使用应用 API Key：`Authorization: Bearer <APPLICATION_KEY>`。

下面配置对应 OpenLIT TypeScript **1.15.0** 与 Python **1.45.0**。在需要观测的 SDK 调用前初始化，并遵守框架要求的初始化和 import 顺序。OpenLIT 提供被观测 SDK 中可获得的事实，不自动采集所有真人消息、Agent 工具或任务终态。

### TypeScript

```typescript
import OpenLIT from 'openlit';

OpenLIT.init({
  otlpEndpoint: 'https://YOUR_HOST/api/logs/otlp',
  otlpHeaders: { Authorization: 'Bearer <APPLICATION_KEY>' },
  disableMetrics: true,
  disableEvents: true,
});
```

该 SDK 使用 HTTP JSON Trace exporter，并在 `otlpEndpoint` 后追加 `/v1/traces`。不要在根地址中重复填写这一后缀。

### Python

```python
import os

# 在 import OpenLIT 前设置，确保选择 HTTP exporter。
os.environ['OTEL_EXPORTER_OTLP_PROTOCOL'] = 'http/protobuf'
os.environ['OTEL_EXPORTER_OTLP_TRACES_ENDPOINT'] = (
    'https://YOUR_HOST/api/logs/otlp/v1/traces'
)

import openlit

openlit.init(
    otlp_headers={'Authorization': 'Bearer <APPLICATION_KEY>'},
    disable_metrics=True,
    disable_events=True,
)
```

Python 未配置 gRPC 时默认使用 HTTP protobuf exporter。上面的 Trace 专属端点是完整 URL。执行真实的被观测调用后，在进程退出前等待 SDK batch exporter 完成 flush，再到应用日志查看记录与轨迹。

OpenLIT CLI 可同时发送 Metrics 和 Trace。其他信号应分别配置到支持的后端；将 CLI 通用 endpoint 指向这里，不能证明 CLI 全部遥测能力可用。

## HTTP contract

以下四个接口都使用 `POST /api/console/applications/{id}/logs/`，要求已登录工作区 session、`x-csrf-token` 和各自独立操作权限。`{id}` 是源或目标应用 ID，不是 Langfuse 项目 ID。

| 后缀 | JSON 请求 | 成功响应 |
| --- | --- | --- |
| `otlp/export` | `{ "log_conversation_ids": ["CONVERSATION_ID_1", "CONVERSATION_ID_2"] }` | 下载 `application/json` 标准 OTLP 对象，不套 `ApiSuccess` |
| `otlp/import` | `{ "otlp": { "resourceSpans": [] } }` | `ApiSuccess<{accepted_events, duplicate_events, record_ids}>` |
| `langfuse/export` | `{ "log_conversation_ids": ["CONVERSATION_ID_1", "CONVERSATION_ID_2"], "base_url": "https://YOUR_LANGFUSE_HOST", "public_key": "<PUBLIC_KEY>", "secret_key": "<SECRET_KEY>" }` | `ApiSuccess<{exported_records, exported_spans, trace_ids}>` |
| `langfuse/import` | `{ "trace_ids": ["TRACE_ID_1", "TRACE_ID_2"], "base_url": "https://YOUR_LANGFUSE_HOST", "public_key": "<PUBLIC_KEY>", "secret_key": "<SECRET_KEY>" }` | `ApiSuccess<{accepted_events, duplicate_events, record_ids}>` |

两个导出接口要求 `record_ids` 与 `log_conversation_ids` **恰好提供一个非空数组**。`record_ids` 精确选择任务记录，保留原接口行为；`log_conversation_ids` 选择会话全部任务，页面使用此字段。成员归属验证与全部来源事实读取在同一个 repeatable-read snapshot 中完成；包含跨应用、缺失或原生记录的选择整组拒绝。重复 ID 去重。

`ApiSuccess` 将回执包在 `{"data": {...}, "meta": null}` 中。空的 `resourceSpans` 仅说明请求形状，导入事件需使用真实 Trace 数据。公开 SDK 上报返回标准 OTLP Trace 响应，不使用 Console 回执 envelope。

既有 `/api/logs/v1/events` 原生上报、collector 协议和原生归档行为保持独立。

### 接收开关与部分导入

停用「日志接收」会阻止新的 Console 导入和 SDK 上报；已有历史记录仍可导出。

Console 导入先收集、映射全部来源数据，再预校验所有 source batch，然后开始持久化。每个 source batch 原子提交，但整个多来源导入不是单一事务。后续批次失败且此前已有记录提交时，服务端返回 HTTP **409**，`code` 为 `agent_logs_partial_import`。此前提交保留。错误响应不包在 `ApiSuccess` 中，包含 `status`、`code`、`message`、`details`：

```json
{
  "status": 409,
  "code": "agent_logs_partial_import",
  "message": "Earlier source batches persisted. Retry unchanged source facts to finish the import safely.",
  "details": {
    "accepted_events": 2,
    "duplicate_events": 1,
    "committed_records": 1
  }
}
```

示例数字表示已提交进度；`committed_records` 是记录数，不是 ID 数组。用未改变的来源事实重试可安全补完，已提交身份会去重，不重复计入新用量。尚无记录提交时的失败返回原始错误。

公开 SDK OTLP 上报遵循 OTLP 规范的 `partial_success` 响应语义，与 Console 的 HTTP 409 错误及其回执详情分开。


## 交换能保留什么

1flowbase 自身导出在 metadata 的 `1flowbase_agent_log` 字段保存带版本的原始 source event，格式为 `1flowbase.agent-logs.source-fact/v1;base64,<JSON 的标准 Base64>`。使用单一字符串可经过 Langfuse 的 metadata 展平，同时保留带点键名、嵌套对象、数组、空值等原始结构。重新导入时保留 source/session/task/event 身份、raw 事实和 usage。此能力依赖 metadata 原样保留；若 Langfuse masking 或其他转换移除、改变它，就不承诺原生无损恢复。

完整的旧嵌套 OTLP 来源封装仍可导入。已被旧版 Langfuse 展平成 `1flowbase_agent_log.*` 的封装会明确拒绝，需从原始来源重新导出；不能通过拆点键名猜测恢复。

普通外部 Trace 可能只有一次 LLM 请求的历史 Context，不能据此宣称完整真人会话或真实任务终态。缺失的模型、用量和完成事实保持缺失；导入费用遵循后端已有价格规则进行估算，不扣余额。Agent Logs 根据来源提供的任务终态结束任务；只有 LLM 请求 Span 时，记录仍可能显示 `running`。OTLP 交换不是数据库备份。

架构分为三层：

```text
OTLP / Langfuse 映射与交换                 协议层
                    ↓
source / session / task / event / usage / raw + 持久化
                                         统一语义层
                    ↑
原生 collector adapter / OpenLIT          来源适配层
```

通用来源恢复留在共享 collector SDK，不复制到每个插件。现有原生 collector 无需为交换功能升级，collector 插件保持不变。

后端协议层新增显式声明 `opentelemetry-proto` **0.33.1**，用于生成的 Trace 消息及官方 serde 表达；`prost` **0.14.1**，用于 protobuf 编解码；`tonic-types` **0.14.1**，用于官方 `google.rpc.Status` 错误表达。提供默认 features 的协议依赖关闭默认 features，不初始化 gRPC server、transport 或 telemetry provider。lockfile 同时新增协议包的间接依赖 `opentelemetry` 与 `opentelemetry_sdk` **0.33.1**，以及 `tonic` **0.14.6**、`prost-types` **0.14.1**，但不初始化 instrumentation runtime。`base64` 复用已有直接依赖，用于来源事实编码。OpenLIT **1.15.0** 只用于临时互通测试工具，不是 1flowbase 产品依赖；上面的 SDK 示例配置的是独立客户端应用。

## 可选的网络中心路由

Langfuse 导入、导出由 1flowbase 服务端发出外部请求。部署需要代理或其他已配置出站路由时，可在「网络中心」配置「Agent Logs 日志交换」，consumer 标识为 `agent_logs_exchange`。该路由配置是可选项，与 Langfuse 项目地址和凭据分开配置。本地 OTLP 文件交换不要求连接远端 Langfuse；OpenLIT 入站 Trace 仍使用前述公开 HTTP receiver。

## 常见问题

| 现象 | 核对 |
| --- | --- |
| SDK 上报被拒绝 | 当前应用 Bearer Key、日志接收已开启、准确的 `/api/logs/otlp/v1/traces` 路径和支持的 HTTP content type |
| Console 交换无权限 | 工作区 session、CSRF token、当前交换操作权限；collector Key 不足以授权 |
| Langfuse 交换失败 | 部署根地址、项目密钥对，以及 1flowbase 服务端到 Langfuse 的连通性 |
| 导入后不是完整对话 | 检查源 Span 与 metadata；LLM 历史消息可能只是 Context |
| 导出任务数与所选行数不同 | 页面选择完整会话，一条会话可包含多个任务；核对服务端任务数、Span 数及 Trace ID |
| Trace 成功但 Metrics 失败 | 此处只接收 Trace；关闭其他信号或发送到支持的后端 |

## 相关文档

- [原生采集协议与恢复](README.zh-CN.md)
- [原生客户端接入](native-clients.zh-CN.md)
- [Codex / Claude Code 本地日志教程](https://github.com/taichuy/1flowbase/wiki/Agent-Logs-Local-Agents-CN)
- [Langfuse OpenTelemetry 接入](https://langfuse.com/docs/opentelemetry/get-started)
- [OpenLIT SDK 源码](https://github.com/openlit/openlit/tree/main/sdk)
