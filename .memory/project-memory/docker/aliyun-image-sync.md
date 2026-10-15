---
memory_type: project
topic: 独立阿里云镜像同步与国内单文件部署
summary: 用户确认前后端与 PostgreSQL 分三个仓库；独立 Action 已推 dev，镜像与双语 Wiki 已发布，默认分支自动触发待上线。
created_at: 2026-10-14 10
updated_at: 2026-10-14 12
decision_policy: verify_before_decision
status: awaiting-default-branch-integration
---

# 范围与状态

Root 在 taichuy-dev 的 `/home/taichuy/git/1flowbase`，主分支保持 dev；任务提交 `7aced36d54bdcd7a7abff5acea9f32ef584770e8` 已推 origin/dev。独立同步不修改 container images 或 GitHub Release 流程，失败不影响原发布。未向 main/latest 合并；工作流和脚本需进入默认 main 才能自动触发。无固定截止日期。Issue https://github.com/taichuy/1flowbase/issues/2435 。用户最新决定将官方 PG 一并镜像到自己的 postgres 仓库，覆盖早先只查现成镜像的决定。

# 发布与验收入口

前后端目标分别为 `taichuy/1flowbase-api-server`、`taichuy/1flowbase-web`，均已发布 v0.5.6/latest；PG 目标 `taichuy/postgres` 已发布 18.6-alpine/18-alpine。使用用户指定的广州个人版 ACR；三种镜像均保留官方 linux/amd64 和 linux/arm64 runtime manifest 摘要，排除个人版拒收的 OCI attestation。索引重组，不重新编译应用。

Variables 已设置 ACR_REGISTRY、ACR_API_REPOSITORY、ACR_WEB_REPOSITORY、ACR_POSTGRES_REPOSITORY、POSTGRES_SOURCE_IMAGE、ACR_USERNAME、ACR_SYNC_ENABLED；密码为 ACR_PASSWORD Secret。旧 ACR_REPOSITORY 已移除。账号与目标路径可在 GitHub 设置修改。

中英文 Wiki 已发布，提交 d42d7ec：https://github.com/taichuy/1flowbase/wiki/Docker-Aliyun-Deployment-CN ，英文页去掉 -CN。完整 YAML 与 Wiki 下载文件、仓库 compose.aliyun.yaml 一致；只放 compose.yaml 的空目录匿名拉取并启动成功，init=0、db healthy、PG18.6、health200、root console/me200；force-recreate 后配置 hash 与数据库 system_identifier 不变。临时测试项目使用33101，原33100实例不受影响。

15 项定向测试、actionlint、Compose 配置与语法检查通过。验收总入口 `tmp/test-governance/aliyun-split-qa.json`，同步报告 `aliyun-split-report.json`、持久化证据 `aliyun-split-persistence.json`、Wiki 回读 `aliyun-wiki-published.json`。原发布工作流未修改，GitHub 自动触发尚未实际运行。
