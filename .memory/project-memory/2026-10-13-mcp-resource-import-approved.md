---
title: "MCP 双向文件传输底座能力与 SSH 插件职责边界"
memory_type: project
created_at: "2026-10-13 15"
updated_at: "2026-10-13 18"
decision_policy: verify_before_decision
status: approved
tags: [mcp, ssh, file-table, runtime-extension-sdk]
---

- 用户批准底座提供资源导入与共享 SDK，SSH 仅通过工具传 resource_uri 与 file_table_id；通用身份、来源授权、资源读取和目标文件保存归底座，插件不重复维护上传和通信协议。
- 首版来源只支持底座生成的 flowbase-resource://read/... 可重放定位 URI；URI 不赋予权限，导入时使用真实调用者与选定 MCP 实例重新授权。HTTP URL 下载、独立传输服务与 AI Gateway 不在此次范围。
- 用户授权真实读取 4090 test-image.jpg 并写入文件表 demo；页面 ID 与文件表 ID 不可混用。允许本地插件提交、升版本、打包安装验证后推私有仓库云端打包，底座集成 dev。7600 及 gateway 工作树进程保持运行。
- 动机是让任意符合共享 SDK 契约的插件复用同一套文件入库能力，保证插件简单并维护当前功能完整性。截止日期未指定；下一阶段仍须以当前源码/运行态核实范围。
- 决策与验收入口：[Root #2410](https://github.com/taichuy/1flowbase/issues/2410)。
- 后续用户明确批准直接实现文件表 → SSH 下载，并延续本地验证后私有云发布的授权；底座负责真实调用者授权读取，共享SDK负责传输，SSH负责远端提交，默认拒绝覆盖。真实远端验证只复用本任务自建目标，保留原图片与demo记录。
- 下载阶段验收真值：[Root #2415](https://github.com/taichuy/1flowbase/issues/2415)，当前已完成统一QA并进入用户验收；两项Delivery已结算。下次继续以该Root当前状态和验收证据核实，不重复执行已有有效验证，不把六平台归档证明当作六OS实机证明。
