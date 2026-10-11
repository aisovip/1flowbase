---
date: 2026-09-24 17
feedback_category: interaction
decision_policy: direct_reference
---

# Agent Logs Capture: Favor Small Universal Integration

- 规则：评估 agent-logs 双向流量采集时，优先比较普适性和对现有 `NGINX → 7800` 路径的最小改动；对 Langfuse、LiteLLM 等完整网关或观测系统，先说明新增链路和运行成本，再判断是否合适。用户提出 mitmproxy 时，具体评估它的反向代理和流式采集边界。
- 原因：用户认为引入 Langfuse 或 LiteLLM 太重，希望用较小的通用采集层满足请求、响应和入库需求。
- 适用场景：agent-logs 的架构决策、NGINX 流量镜像与请求/响应采集方案比较。

## Prefer Existing Agent Lifecycle Hooks (2026-10-11 13)

- 规则：Agent 已提供可用的原生生命周期 Hook 时，优先将日志采集接入 Hook，以按需短任务执行；不要默认再起独立常驻进程扫描会话。历史补采可以保留显式入口，共享上传、断点和恢复仍由通用采集 SDK 承担。
- 原因：用户明确否定 Codex 已开放 Hook 后仍常驻轮询的设计，要求降低额外进程和重复扫描成本。
- 适用场景：Codex 及其他具备原生 Hook 的 Agent 采集器设计；没有等价事件或持久日志时先说明能力边界，不伪造持续采集保证。
