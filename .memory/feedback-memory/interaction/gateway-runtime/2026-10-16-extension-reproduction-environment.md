---
title: 扩展安装故障需在用户实际环境复现
memory_type: feedback
feedback_category: interaction
created_at: "2026-10-16 08"
updated_at: "2026-10-16 08"
decision_policy: direct_reference
---

- 规则：用户报告 gateway 扩展安装或启用故障时，在其授权范围内用浏览器重走实际卸载、安装、启用流程，逐步保存 HTTP 状态、错误码与页面反馈；dev 的成功不代表 gateway 已修复。区分安装成功、权限具备、启用成功及提示改进。
- 原因：dev SSH 安装保留了贡献授权，而 gateway 同版本授权为空；仅在 dev 验证并改善错误提示未闭合 gateway 的用户流程，用户因此要求现场复现。
- 适用场景：同源码但授权、安装历史、运行配置不同的环境。操作授权以当前请求为准，本轮允许 gateway 浏览器卸载重装，不授权重启 7600。
