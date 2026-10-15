---
title: 新增通知渠道恢复抽屉内类型选择
date: 2026-10-15 23
decision_policy: verify_before_decision
---

- 用户确认平衡方向：新增直接打开抽屉，内部下拉选择通知类型；未保存时切换保留渠道名称、清空类型专属配置；保存后类型固定，取消和保存按钮留在底部。
- 原因：此前type-first新增菜单移除了抽屉内切换，用户要求恢复；系统通知归底座，插件通知配置和授权由各类型声明，微信扫码业务归插件。
- 当前在taichuy-dev `/home/taichuy/git/1flowbase` dev工作区实现，HEAD基线bba052b8b；保留已有未提交UI布局改动，未整体提交这些其他任务改动。无新增截止日期。
- 本次32项相关测试通过，定向lint无error、保留1条既有memo依赖warning；浏览器验证真实系统/微信类型切换与桌面/手机底部操作区。证据 `tmp/test-governance/notification-type-selector/qa-result.json`。
- 未改后端/插件协议，未新增通知实例，未重启服务。后续取证前以当前源码和运行态复核。
