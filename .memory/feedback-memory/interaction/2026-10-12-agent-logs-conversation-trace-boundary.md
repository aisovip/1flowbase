---
memory_type: feedback
feedback_category: interaction
topic: Agent Logs 主会话与子代理的三级日志边界
summary: 一个主会话统一归档；二级只呈现主对话系统提示词、用户输入、AI最终回复，最近五轮滚动；子代理及其执行过程属于三级轨迹，不能在二级追加子代理会话卡片。
keywords: [agent-logs, conversation, subagent, trace, 三级日志]
match_when:
  - 设计或调整采集日志会话归属、会话历史、子代理展示及AI日志读取
created_at: 2026-10-12 00
updated_at: 2026-10-12 00
last_verified_at: 2026-10-12 00
decision_policy: direct_reference
scope: [agent-logs, application-logs]
---

# 主会话统一归档，子代理仅在轨迹中查看

## 规则

- 一个主会话是一个归档单位，subagent属于该会话；保留来源标识和父子执行关系。
- 二级会话历史只读取主对话：系统提示词、用户输入、AI最终回复；默认最近五轮并可滚动向前读取。
- 子代理对话、工具调用与执行过程在对应主轮次的三级轨迹中查看，支持继续下钻。
- 不通过在二级主对话追加可折叠子代理卡片来冒充会话聚合；后端会话归属、UI与AI读取契约须一致。

## 原因

用户否定了把父子关联转换为二级子代理对话卡片的方案。二级用于理解人与主代理的交互，三级用于查看产生该回复的执行过程。两种语义混合会破坏统一日志查询与阅读路径。

## 适用场景

Codex、Claude Code、DeepSeek Harness、OpenCode等采集记录归档，以及主流程和子代理日志的展示、查询与MCP接口。
