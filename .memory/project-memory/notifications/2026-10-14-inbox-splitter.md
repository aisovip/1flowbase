---
memory_type: project
topic: 通知收件箱使用 Splitter 与浏览器比例记忆
summary: 用户批准平衡方案并要求轻量保存比例、渠道名称前置、按钮集中与消息已读按钮无边框；使用7800并保留数据。
created_at: 2026-10-14 12
updated_at: 2026-10-14 12
decision_policy: verify_before_decision
status: user_acceptance
keywords: [notifications, splitter, browser-storage, inbox]
---

- 谁在做什么：Root 实施通知铃铛抽屉内的左右分隔面板，承接用户于本聊天批准的平衡方向。
- 为什么这样做：静态分隔不能调节渠道与消息阅读空间；用户希望参考既有 Splitter 使用经验，浏览器只存少量数值，避免重复调整。
- 已批准边界：宽屏可拖动、两侧最小宽度、左右独立滚动、窄屏渠道与消息切换；用户新增要求保存比例、渠道名称前置、工具栏动作集中、消息内已读/未读按钮无外围轮廓。使用本地 API 7800，已有数据不删除。
- 为什么需要：减少阅读空间限制与内部按钮视觉噪音，保持移动端标题和动作关系清晰。
- 截止日期：未设置日历期限；完成开发验证后仍由用户验收视觉效果。
- 证据入口：tmp/test-governance/notification-splitter/qa-report.md 与当前页面；截图中“验收渠道”是浏览器只读 fixture，不是新增数据库记录。使用状态前核对当前源码和运行证据。
