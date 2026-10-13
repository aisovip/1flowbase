---
title: "文件模板与真实上传 Demo 已批准范围"
memory_type: project
created_at: "2026-10-13 00"
updated_at: "2026-10-13 10"
decision_policy: verify_before_decision
status: active
tags: [files, templates, frontstage]
---

用户于本轮确认平衡方案，Root 实现主数据源 core/file/v1 文件表模板；创建可选 bound_storage_id，null 每次上传解析当前默认，旧文件保留实际 storage_id。后端拥有文件元数据与存储语义，前端复用共享 DataTable 和 AntD Dragger。既有 attachments 不迁移模板/绑定；明确绑定沿用 root/system 权限。

目的是恢复建表入口缺失的文件表能力，并在 `/tasks/pages/01a13087-6eb9-78e0-b853-670173c44396` 提供真实上传后刷新文件记录的 Demo。Root Issue #2405（Delivery #2406/#2407）记录当前范围与验收；没有用户指定截止日期，本轮完成后交用户验收。

当前开发基线：taichuy-dev `/home/taichuy/git/1flowbase`，dev d022bb62。7600 是观察样本；任何 gateway 分支 worktree 的进程不可停止或重启。运行与交付状态必须重新核对，不凭这条记忆判断已集成/推送/部署。

2026-10-13 验收补充：主开发环境真实创建 file_demo（null binding），指定页面已经配置 FileTableView。真实上传201、立即刷新、下载内容比对、默认切换/显式绑定/旧文件读取通过；既有attachments绑定保持。文件模板记录绕过普通CRUD的协议级记录缓存，直接读持久化存储，避免上传后读到旧列表。最终代码与运行态以Root #2405验收记录为准，不据此推断远端CI或生产部署状态。

2026-10-13 09 方案补充：用户明确批准 #2408 Single Issue 并要求实现稳定站内相对 url 与临时公开按钮。临时有效期内任意持链接者匿名读取，不校验接收人、不重新验证签发人；短期授权使用 ephemeral TTL，不新增持久化授权表。既有 attachments 不改模板/存储绑定，但注册文件表的 path/url metadata 与相对地址纳入明确迁移。Root 已完成定向 QA 与主开发 7800 运行更新，Issue #2408 等待用户验收，未关闭；无指定截止日期。当前事实与实际提交/远端/部署状态重新查 Issue、git 与进程，不凭记忆推断。


2026-10-13 10 图片预览扩展：用户明确批准 local_default 原图预览方案并要求直接实现。Root 沿用 #2408 Single Issue；/settings/files 的本地存储增加默认开启的 image_preview_enabled，preview_url 是原生只读计算字段，不创建物理列，由后端在授权后的通用 list/get 和上传响应投影。当前安全栅格图片使用同源相对预览 URL，下载 attachment 与预览 inline 分开；SVG/HTML 不作内联预览。临时公开链接可以返回独立 preview_url，仍以 ephemeral TTL 授权，底座管 URL/权限/失效，驱动读对象。未加入缩略图任务或未来插件预览生成平台。动机是表格直接预览并点击放大，同时保留存储和访问边界；没有指定截止日期。最终部署/提交事实应查 #2408、git 与实际进程，不能从本记忆推断。
