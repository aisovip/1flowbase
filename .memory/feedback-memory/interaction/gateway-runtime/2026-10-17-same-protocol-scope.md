---
date: 2026-10-17 00
feedback_category: interaction
decision_policy: direct_reference
summary: 同协按接口标准判断并覆盖三协议；用户因改动规模、维护成本和稳定性风险明确放弃本次改造。
---

# Same Protocol Passthrough Scope

- 规则：用户所说“同协”指客户端与上游使用相同请求接口标准：Responses 对 Responses、Anthropic Messages 对 Messages、Chat Completions 对 Chat Completions。设计第二层 AI Native 的透传契约时覆盖三种协议，不擅自把目标收窄为 Responses；同协判定与本次调用是否需要工作流加工分开讨论。
- 原因：上一轮评估最终只建议优先完善 Responses，用户指出网关不可能仅兼容一种，并明确 SYSTEM、历史和 MCP 加工是需要权衡的功能完整性问题。
- 适用场景：AI Gateway 同协透传、协议上下文配置和 AI Native 双表示设计。
- 决策更新（2026-10-17）：用户明确放弃本次同协透传改造，原因是改动过大、维护成本高且容易引入严重问题。保留现有架构与协议上下文行为，不继续实施或默认恢复该提案；本轮只做了源码评估，没有产品代码变更需要回滚。
