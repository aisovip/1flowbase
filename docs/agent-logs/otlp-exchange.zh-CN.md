# 使用 OTLP 与 Langfuse 交换 Agent Logs

[English](otlp-exchange.md)

> 本地文档草稿：实际截图和互通验证待集中验收。下方图片 URL 为真实页面截图预留位置，不代表交换已验证成功。

可以将多条日志导出为 OTLP JSON，在兼容系统间交换；也可以把多条日志发送到 Langfuse 项目，或把多个 Langfuse Trace 导入指定 1flowbase 应用。现有原生采集器的安装、事件上传和恢复行为保持独立。

## 准备应用与凭据

创建 **Agent Logs** 应用；需要 SDK 上报时，创建该应用的 API Key，并启用「日志接收」。Console 交换需要登录正确工作区，并拥有对应操作权限。应用 Key 授权 Trace 上报，不授权 Console 交换。

Langfuse 需要项目的 **Public Key**、**Secret Key** 和部署根地址，例如 `https://YOUR_LANGFUSE_HOST`。填写部署地址，不填写 Trace 页面或 `/api/public/otel/v1/traces`。1flowbase 服务端必须能访问该部署。交换窗口只在当前内存中保留凭据，关闭后清空；Secret Key 遮罩显示。

日志可能包含提示词、代码和工具输出，请选择实际需要共享的记录与目标。

## 选择多条记录，导出 OTLP JSON

1. 进入应用的「日志」页面，勾选多条记录。
2. 点击「日志交换」，选择「导出 OTLP JSON」，再点击「执行交换」。
3. 保存下载的 JSON 文件。它是标准 OTLP Trace 数据，不是平台备份或原生采集器归档。

在全局日志页面，跨应用选择会按应用分别导出。导入没有隐含的跨应用目标，必须明确选择目标应用。

![选择多条日志并打开日志交换](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/01-record-selection.png)

文件导入时，在「日志交换」选择「导入 OTLP JSON」，选择内容为 OTLP JSON 对象的文件，然后执行。核对服务端回执：`accepted_events`、`duplicate_events` 是事件数，`record_ids` 是生成的日志记录 ID。事件数不等于对话数。

## 导出多条记录到 Langfuse

1. 勾选记录，打开「日志交换」，选择「导出到 Langfuse」。
2. 填写目标项目的「Langfuse 地址」、Public Key 和 Secret Key。
3. 执行后核对 `exported_records`、`exported_spans`、`trace_ids`。若导出数少于所选记录数，先调查部分结果，不能认为所有记录已到达。
4. 在 Langfuse 打开同一项目，根据回执中的 Trace ID 定位，逐项查看输入、输出、Span 和 metadata 是否与源记录对应。

![填写多记录 Langfuse 导出配置](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/02-langfuse-export.png)

![在目标 Langfuse 项目定位导出的 Trace](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/03-langfuse-traces.png)

服务端使用 Public Key 和 Secret Key 组成 HTTP Basic 认证，向 Langfuse 的 `/api/public/otel/v1/traces` 发送 OTLP Trace。这些是 Langfuse 项目凭据，不是 1flowbase 应用 Key。

## 从 Langfuse 导入多个 Trace

1. 进入目标应用「日志」页面；在全局日志页面时明确选择导入目标应用。
2. 在「日志交换」选择「从 Langfuse 导入」，填写部署地址和项目凭据。
3. 粘贴多个 Trace ID，用逗号或换行分隔，执行交换。
4. 核对 `accepted_events`、`duplicate_events`、`record_ids`，打开对应记录查看导入的对话与轨迹。同一身份且内容不变的重传计为重复；同一身份下内容冲突会报错。

![指定目标并填写多个 Langfuse Trace ID](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/04-langfuse-import.png)

![查看导入记录的对话](https://raw.githubusercontent.com/wiki/taichuy/1flowbase/assets/agent-logs-otlp/05-imported-conversation.png)

远程导入读取 Langfuse public traces 和 observations，包含全部 observation 分页。支持 observations-v2 API 和 legacy API mode；只有明确的能力不存在响应（404）才选择 legacy 能力。认证、网络或服务端错误需直接处理，不能解释为旧版 API 信号。

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
| `otlp/export` | `{ "record_ids": ["RECORD_ID_1", "RECORD_ID_2"] }` | 下载 `application/json` 标准 OTLP 对象，不套 `ApiSuccess` |
| `otlp/import` | `{ "otlp": { "resourceSpans": [] } }` | `ApiSuccess<{accepted_events, duplicate_events, record_ids}>` |
| `langfuse/export` | `{ "record_ids": ["RECORD_ID_1", "RECORD_ID_2"], "base_url": "https://YOUR_LANGFUSE_HOST", "public_key": "<PUBLIC_KEY>", "secret_key": "<SECRET_KEY>" }` | `ApiSuccess<{exported_records, exported_spans, trace_ids}>` |
| `langfuse/import` | `{ "trace_ids": ["TRACE_ID_1", "TRACE_ID_2"], "base_url": "https://YOUR_LANGFUSE_HOST", "public_key": "<PUBLIC_KEY>", "secret_key": "<SECRET_KEY>" }` | `ApiSuccess<{accepted_events, duplicate_events, record_ids}>` |

`ApiSuccess` 将回执包在 `{"data": {...}, "meta": null}` 中。空的 `resourceSpans` 仅说明请求形状，导入事件需使用真实 Trace 数据。公开 SDK 上报返回标准 OTLP Trace 响应，不使用 Console 回执 envelope。

既有 `/api/logs/v1/events` 原生上报、collector 协议和原生归档行为保持独立。

## 交换能保留什么

1flowbase 自身导出会在 metadata 嵌入带版本的原始 source event，重新导入时保留 source/session/task/event 身份、raw 事实和 usage。此能力依赖 metadata 原样保留；若 Langfuse masking 或其他转换移除、改变它，就不承诺原生无损恢复。

普通外部 Trace 可能只有一次 LLM 请求的历史 Context，不能据此宣称完整真人会话或真实任务终态。缺失的模型、用量和完成事实保持缺失；导入费用遵循后端已有价格规则进行估算，不扣余额。OTLP 交换不是数据库备份。

架构分为三层：

```text
OTLP / Langfuse 映射与交换                 协议层
                    ↓
source / session / task / event / usage / raw + 持久化
                                         统一语义层
                    ↑
原生 collector adapter / OpenLIT          来源适配层
```

通用来源恢复留在共享 collector SDK，不复制到每个插件。后端协议层使用 `opentelemetry-proto` **0.33.1** 与配套 `prost` 依赖解码 protobuf。现有原生 collector 无需为交换功能升级，本轮不修改 collector 插件。

## 常见问题

| 现象 | 核对 |
| --- | --- |
| SDK 上报被拒绝 | 当前应用 Bearer Key、日志接收已开启、准确的 `/api/logs/otlp/v1/traces` 路径和支持的 HTTP content type |
| Console 交换无权限 | 工作区 session、CSRF token、当前交换操作权限；collector Key 不足以授权 |
| Langfuse 交换失败 | 部署根地址、项目密钥对，以及 1flowbase 服务端到 Langfuse 的连通性 |
| 导入后不是完整对话 | 检查源 Span 与 metadata；LLM 历史消息可能只是 Context |
| 导出数小于选择数 | 对照服务端回执和返回 ID，逐应用处理失败分组后再重试 |
| Trace 成功但 Metrics 失败 | 此处只接收 Trace；关闭其他信号或发送到支持的后端 |

## 相关文档

- [原生采集协议与恢复](README.zh-CN.md)
- [原生客户端接入](native-clients.zh-CN.md)
- [Codex / Claude Code 本地日志教程](https://github.com/taichuy/1flowbase/wiki/Agent-Logs-Local-Agents-CN)
- [Langfuse OpenTelemetry 接入](https://langfuse.com/docs/opentelemetry/get-started)
- [OpenLIT SDK 源码](https://github.com/openlit/openlit/tree/main/sdk)
