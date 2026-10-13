---
title: "扩展上传弹窗采用文件列表，去掉补充包信息折叠"
memory_type: feedback
feedback_category: interaction
created_at: "2026-10-13 23"
updated_at: "2026-10-13 23"
decision_policy: direct_reference
status: active
tags: [extension-center, upload, antd]
---

- 规则：扩展中心上传弹窗按用户参考图采用名称、大小、状态、操作的文件列表，并移除补充包信息折叠；选文件后明确开始上传，每行显示真实进度与安装结果。
- 原因：用户明确否定额外信息折叠，要求上传过程与选中文件建立直接对应，避免用多余表单增加操作负担；插件清单信息由后端识别。
- 适用场景：本项目扩展中心的插件归档上传，不据此删其他专用JSON导入所需的信息或改变后端安装契约。验收入口：https://github.com/taichuy/1flowbase/issues/2418。
