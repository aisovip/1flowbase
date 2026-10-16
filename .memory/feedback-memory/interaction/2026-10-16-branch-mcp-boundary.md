---
title: dev 与 gateway 的 MCP 环境边界
date: 2026-10-16 22
feedback_category: interaction
decision_policy: direct_reference
---

开发 dev 分支时使用 Codex 本地的 1flowbase MCP，不使用连接器。1flowbase-gateway 连接器对应 gateway 分支。
原因：两个入口连接不同分支的运行环境及数据，混用会定位或修改错误对象。
适用：1flowbase 开发、读取或修改低代码区块和 MCP 数据。若本地 MCP 未暴露，明确说明工具缺口，不用 gateway 连接器替代。
