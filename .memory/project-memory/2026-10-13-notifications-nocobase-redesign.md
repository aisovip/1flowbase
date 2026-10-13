---
memory_type: project
topic: 通知管理按 NocoBase 平衡方案改版
summary: 用户确认仅复刻紧凑列表、筛选、类型优先新增和配置抽屉；新增数据能力不在范围内。
created_at: 2026-10-13 23
updated_at: 2026-10-13 23
decision_policy: verify_before_decision
status: user_acceptance
keywords: [notifications, nocobase, frontend, settings]
---

- 谁在做什么：Root 承接 #2419；用户确认平衡方向，允许只读子 agent 使用 gpt-6.1-sol medium 核对 `/home/taichuy/git/nocobase` 源码。
- 为什么这样做：旧通知页虽功能验收通过，用户不接受其交互和审美，要求以真实 NocoBase 渠道页为参考。
- 决策边界：改 `/settings/notifications` 的内容区和新增/编辑路径；沿既有 DTO、插件表单、秘密保护和启停删除能力。筛选归后端列表 owner。不新增描述、自定义标识或发送日志，不修改全站壳层、铃铛、工作流，不云端部署。
- 为什么需要：让列表更紧凑，新增先选类型再配置，避免高空白工具栏与无类型的初始表单。
- 截止日期：没有另设日历期限；实现后进入用户验收，尚未获得最终审美验收，不能自动关闭 Issue。
- 复核入口：#2419；本地 `tmp/test-governance/notifications-redesign/qa-report.md` 和当前页面。状态使用前核对当前源码、Git 与运行态，不以本条代替证据。
