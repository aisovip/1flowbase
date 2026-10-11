# 原生客户端日志采集

[English](native-clients.md)

采集器由客户端原生 Hook 或插件回调触发，共用 SDK 的私有配置、待发送队列、稳定事件 ID、增量断点和 ACK 校验。安装不会扫描或上传全部历史，也不需要独立常驻监听进程。

| 客户端 | 接入 | 版本 |
| --- | --- | --- |
| Codex | 原生 Hook 定位当前 transcript | 0.4.0 |
| Claude Code | settings Hook 定位当前 transcript | 0.1.0 |
| DeepSeek Harness | Cordis 会话事件、轮次结束/checkpoint 回调 | 0.1.0 |
| OpenCode | 默认 V1 message/part 事件，兼容 V2 session 事件 | 0.1.0 |

DSH/OpenCode 回调返回前同步持久化原生事件，idle/checkpoint/turn-end 通知触发投递。idle 本身不能证明成功。投递失败保留队列，后续回调或显式 flush 重试。

## 配置与恢复

签名版本发布后可使用 Agent Logs 的应用专属安装命令；本地可执行文件也支持：

~~~sh
COLLECTOR configure --endpoint https://HOST/api/logs/v1/events --config /PRIVATE/config.json --source /PRIVATE/source --key-stdin
COLLECTOR install-hooks --config /PRIVATE/config.json --client-home /CLIENT/HOME
COLLECTOR flush --config /PRIVATE/config.json
COLLECTOR uninstall-hooks --config /PRIVATE/config.json --client-home /CLIENT/HOME
~~~

COLLECTOR 替换为对应的 CLIENT-logs-collector。Codex 保留 --codex-home。Claude source 指向 transcript home，DSH/OpenCode 指向私有 journal。日志应用密钥从 stdin 输入；模型调用的网关密钥属于客户端配置，用途不同。

注册保留其他 Hook/配置，卸载只移除自身注册，配置和断点保留。import 是显式操作；安装不调用它。Claude 续聊触发 Hook 时可能读取当前会话已有的 transcript。

规范化 input_tokens 包含普通输入、缓存读取和缓存写入。未知来源值保持未知；原生明确省略表示零的计数才补零。完成、失败和取消依据真实终态，最终答案需要成功终态及实际文本。

## 验证配置

目标应用发布版本须声明模型，先核对当前密钥的 /v1/models。

- Claude Code：Anthropic 服务根地址，模型 gpt-6-luna，--effort max。
- DSH：Chat/Responses baseURL 带 /v1，Anthropic 不带。模型设置 reasoningEfforts: {max: max} 和 compat: {forceAdaptiveThinking: true}，以发送 adaptive thinking 和 output_config.effort=max。
- OpenCode 默认 V1：provider.npm 分别为 @ai-sdk/openai-compatible、@ai-sdk/openai、@ai-sdk/anthropic。options 配置 baseURL/apiKey；variants 是对象。Chat/Responses variant 设置 reasoningEffort: max，Anthropic 设置 effort: max 与 thinking: {type: adaptive}，通过 --variant max 选择。

验收核对实际请求、工具结果、续聊和正式日志；退出码或配置名不能单独证明协议及推理强度。后台标题请求与 Agent 轮次分别核对；隔离测试可禁用 OpenCode title agent。

原生接口版本、事件细节和安装限制见官方插件仓库各采集器的中英文 README。
