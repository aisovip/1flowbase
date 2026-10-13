---
memory_type: feedback
feedback_category: interaction
topic: 通知管理改版以 NocoBase 为视觉与交互参考
summary: 当前通知页虽已完成功能验收，用户仍否定其交互和审美，要求参考本地 NocoBase 复刻。
keywords: [notifications, nocobase, ui, interaction]
match_when:
  - 调整通知管理页面的视觉和新增编辑流程
created_at: 2026-10-13 23
updated_at: 2026-10-14 00
last_verified_at: 2026-10-14 00
decision_policy: direct_reference
scope:
  - web/app/src/features/settings/pages/notifications
---

## 规则

通知管理页面改版以用户提供的本地 NocoBase 通知管理界面为视觉与交互参考，先查看真实页面及流程，不能仅沿用原功能验收结论判断用户满意。

通知渠道的启用/停用使用状态列独立开关；操作栏只保留编辑、删除，不再重复放启用/停用文字动作。

## 原因

用户明确指出当前通知中心的设计、交互不符合其偏好，并要求参考 NocoBase 复刻。

2026-10-14 用户指出状态和操作栏的启停表达重复，要求用一个单独开关完成切换。

## 适用场景

本次及后续通知设置页面的视觉与交互调整。该反馈不表示全站改用 NocoBase 壳层，也不自动授权增加参考产品独有的数据字段或功能。
