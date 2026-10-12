---
title: "文件模板与真实上传 Demo 已批准范围"
memory_type: project
created_at: "2026-10-13 00"
updated_at: "2026-10-13 02"
decision_policy: verify_before_decision
status: active
tags: [files, templates, frontstage]
---

用户于本轮确认平衡方案，Root 实现主数据源 core/file/v1 文件表模板；创建可选 bound_storage_id，null 每次上传解析当前默认，旧文件保留实际 storage_id。后端拥有文件元数据与存储语义，前端复用共享 DataTable 和 AntD Dragger。既有 attachments 不迁移模板/绑定；明确绑定沿用 root/system 权限。

目的是恢复建表入口缺失的文件表能力，并在 `/tasks/pages/01a13087-6eb9-78e0-b853-670173c44396` 提供真实上传后刷新文件记录的 Demo。Root Issue #2405（Delivery #2406/#2407）记录当前范围与验收；没有用户指定截止日期，本轮完成后交用户验收。

当前开发基线：taichuy-dev `/home/taichuy/git/1flowbase`，dev d022bb62。7600 是观察样本；任何 gateway 分支 worktree 的进程不可停止或重启。运行与交付状态必须重新核对，不凭这条记忆判断已集成/推送/部署。

2026-10-13 验收补充：主开发环境真实创建 file_demo（null binding），指定页面已经配置 FileTableView。真实上传201、立即刷新、下载内容比对、默认切换/显式绑定/旧文件读取通过；既有attachments绑定保持。文件模板记录绕过普通CRUD的协议级记录缓存，直接读持久化存储，避免上传后读到旧列表。最终代码与运行态以Root #2405验收记录为准，不据此推断远端CI或生产部署状态。
