---
memory_type: project
topic: 微信通知插件与实例扫码授权
summary: 微信通知1.0.1已在3100/7800部署并云端发布；无效发送日志已取证，等待用户扫码和实际收件验收。Root2440保持开放。
created_at: "2026-10-15 00"
updated_at: "2026-10-15 08"
decision_policy: verify_before_decision
status: ready-for-user-scan
keywords: [notifications, wechat, ilink, runtime-extensions, qr, authorization]
---

- 谁在做什么：Root执行用户明确授权的微信通知接入，计划真值为 `https://github.com/taichuy/1flowbase/issues/2440`，Delivery A #2442 / B #2441。用户要求“先弄好”，完成后在3100扫码授权。
- 为什么做：现有通知中心只有内置系统通知，需要微信通知类别与可查看的无效发送日志。
- 已批准边界：通知实例授权与管理员启停独立；Host处理workspace权限、共享managed dispatcher、凭证加密、状态revision和安全发送日志；微信插件仅处理QR/getupdates/sendmessage，不新增聊天、群聊、任意peer或常驻服务。
- 关键事实须复核：腾讯官方实现允许无context直接尝试发送，但会话窗口/供应商拒绝影响实际可发送性。扫码成功不能据此推导实际收件或已读；明确拒绝记failure，无法确认外部效果记unknown。只绑定扫码用户，其他sender不能重绑。
- 当前证据入口：`tmp/test-governance/wechat/root-issue.md`、`research.md`与Root Control Ledger；状态和commit先查当前Git/测试结果，不凭本记忆宣称已合dev/push/deploy。
- 当前阶段：已合dev、push、部署3100/7800，微信通知1.0.1本地安装启用并发布六平台签名包；最终源码、运行与云端证据见 `tmp/test-governance/wechat/delivery-report.md`。后续先核对实时状态，不把此记录当作运行真值。
- 验证与交付：fresh QA集中验证与真实3100二维码/取消/未授权失败日志取证已完成，结论为Ready-for-user-scan。跨workspace/CAS/rekey/selective-backup重型门禁及自动boot fixture未执行；不宣称完整QA_PASS。真实微信扫码与收件仍由用户完成；不要在收尾擅自开始或取消授权。7600 gateway为观察样本，禁止中断或重启。
- 清理与下一步：三个任务worktree及自建QA数据库/临时schema已回收，证据保留。用户进入3100通知管理选择“微信通知”并点击“Scan with WeChat”获取新二维码；历史截图二维码已取消。Root2440等待实际收件验证，不提前关闭。
- 截止日期：未设日历期限；停止条件为目标/接收身份需要扩张、凭证安全不能保证或必要外部动作未到位。
- 动机：复用底座成熟插件架构，避免每个通知供应商重复维护通信协议；让未授权/过期/明确拒绝/未知结果都可解释、可追查。
