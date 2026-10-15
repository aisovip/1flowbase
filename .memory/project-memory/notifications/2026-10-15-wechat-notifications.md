---
memory_type: project
topic: 微信通知插件与实例扫码授权
summary: 微信1.0.2真实发送已验收；用户随后要求改为用户消息开启24小时通知窗口，未发消息或过期须明确失败并记日志，滚动24小时订阅1.1.0已合并推送、部署3100/7800并六平台发布，真实消息窗口与正式发送成功日志已验收。
created_at: "2026-10-15 00"
updated_at: "2026-10-15 21"
decision_policy: verify_before_decision
status: delivered-subscription-window
keywords: [notifications, wechat, ilink, runtime-extensions, qr, authorization]
---

- 谁在做什么：Root执行用户明确授权的微信通知接入，计划真值为 `https://github.com/taichuy/1flowbase/issues/2440`，Delivery A #2442 / B #2441。用户要求“先弄好”，完成后在3100扫码授权。
- 为什么做：现有通知中心只有内置系统通知，需要微信通知类别与可查看的无效发送日志。
- 已批准边界：通知实例授权与管理员启停独立；Host处理workspace权限、共享managed dispatcher、凭证加密、状态revision和安全发送日志；微信插件仅处理QR/getupdates/sendmessage，不新增聊天、群聊、任意peer或常驻服务。
- 关键事实须复核：腾讯官方实现允许无context直接尝试发送，但会话窗口/供应商拒绝影响实际可发送性。扫码成功不能据此推导实际收件或已读；明确拒绝记failure，无法确认外部效果记unknown。只绑定扫码用户，其他sender不能重绑。
- 当前证据入口：`tmp/test-governance/wechat/root-issue.md`、`research.md`与Root Control Ledger；状态和commit先查当前Git/测试结果，不凭本记忆宣称已合dev/push/deploy。
- 当前阶段：原功能已合dev、push、部署；用户扫码后，1.0.2修复真实腾讯成功回包仅含message_id而被误判unknown的问题，插件补丁已合官方仓库main并push；provider-ci和六平台发布全部成功，1.0.2正式Release已发布。证据见 `tmp/test-governance/wechat/authorized-send/cloud-workflow.json` 和 `cloud-release.json`。后续先核对实时状态，不把此记录当作运行真值。
- 验证与交付：本轮15项插件测试通过；正式7800完成通知HTTP200/accepted=true/日志success，用户明确确认测试和完成通知均收到。原有集中QA和失败日志证据保留；跨workspace/CAS/rekey/selective-backup重型门禁及自动boot fixture仍未执行，不宣称完整QA_PASS。证据见 `tmp/test-governance/wechat/authorized-send/report.md`。
- 实例与授权：继续使用原实例；1.0.2通过正式API安装、分配、授权、启用，在用户授权的1flowbase开发库事务内只重绑该实例installation_id，验证授权密文、revision和workspace未改变。不要擅自重新扫码、取消或清除授权。7600 gateway为观察样本，禁止中断或重启。
- 清理与下一步：任务worktree及自建QA数据库/临时schema已回收，证据保留。本轮收件验收已完成；后续使用既有授权渠道发送，不再要求用户扫码。
- 新需求确认（2026-10-15 17）：用户明确确认滚动续期，每次收到绑定用户消息以腾讯create_time_ms重新计算24小时；不能用拉取时间代替。账号授权与窗口独立，过期保留授权；无可验证消息/过期/同步无法确认由插件给出结构化失败，Host返回安全说明并落通知日志。Root #2449（D1 #2450/D2 #2451）为当前计划真值，证据在 `tmp/test-governance/wechat-subscription/`。底座dev@46231cb39、官方插件main@d8f474e已同步；exactCI后端部署7800，1.1.0安装原实例且授权保留。用户新消息真实窗口从消息时间+24h，正式发送accepted=true且日志success；3100桌面/手机取证通过，六平台发布success。最终证据 `tmp/test-governance/wechat-subscription/report.md`；未声称用户终端收件确认或全仓QA。四个task-owned worktree已回收，7600原PID3705587保持。
- 截止日期：未设日历期限；停止条件为目标/接收身份需要扩张、凭证安全不能保证或必要外部动作未到位。
- 动机：复用底座成熟插件架构，避免每个通知供应商重复维护通信协议；让未授权/过期/明确拒绝/未知结果都可解释、可追查。

- 展示确认（2026-10-15 18）：用户要求启用状态、授权状态、通知订阅拆成独立列。已完成并在3100桌面/移动页面取证；不支持授权或订阅的渠道对应列显示“—”。仅布局调整，不改状态来源及插件协议；证据 `tmp/test-governance/notification-status-columns/report.md`。

- 多语言边界确认（2026-10-15 21）：用户明确要求底座内置通知文案由前端翻译，插件文案随插件维护，可参考SSH插件自有词典。此次仅适配展示，不改变用户渠道名、消息值或微信24小时窗口。执行证据入口 `tmp/test-governance/notification-i18n/`、Root #2464；最终合入、部署、发布状态应查报告与当前运行态。

- 多语言交付（2026-10-15 21）：底座dev@75338d4c3已合入并push，7800部署同源云端产物；微信1.1.1本地编译、打包、原实例替换验证，授权与窗口保留，3100中英文菜单/抽屉实测通过并恢复中文。官方插件main@67476b5已同步六平台发布目录，provider-ci/release成功。证据 `tmp/test-governance/notification-i18n/report.md`，当前运行/Git仍需复核，不以记忆替代。
