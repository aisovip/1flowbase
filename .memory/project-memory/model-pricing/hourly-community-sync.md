---
title: "模型价格社区来源与每小时同步授权"
memory_type: project
created_at: "2026-10-14 12"
updated_at: "2026-10-14 12"
decision_policy: verify_before_decision
status: active
tags: [model-pricing, official-plugins, github-actions]
---

用户于 2026-10-14 授权 Codex 在 `/home/taichuy/git/1flowbase-official-plugins` 实现每小时 GitHub Action，同步模型价格并转换成 1flowbase 模型价格格式。采用 Models.dev 为主、LiteLLM 同供应商补缺的已确认方向，目的是减少人工录入和追踪社区基础价格的工作；用户接受用于估算的价格精度。

当前授权只涵盖输入、输出、缓存读取、缓存写入四类默认价格：完整可用则更新，缺失则跳过。上下文、波峰波谷及其他条件规则留到后续集中讨论，本阶段保留既有规则，不自行重算或引入新规则。未指定终止日期；每小时同步持续运行，直到用户另行调整。

后续决策前核对插件仓库 `model-pricing/README.md`、`README.zh-CN.md` 与 `.github/workflows/model-pricing-sync.yml`，以当前源码及实际 Action 运行记录为准；不要将社区数据标为最新厂商核验报价，也不要据此覆盖用户安装后手动编辑的价格。
