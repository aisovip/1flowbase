# Schema Hygiene Rules

## Profiles

- `managed_table` is the default for every unmarked physical table.
- `dynamic_model_table` is for generated workspace-scoped user data tables.
- `registered_system_table` is a fixed physical table registered into metadata.

## registered_system_table

`registered_system_table` means fixed physical table, metadata registration, and read-only field template.

It is not a schema hygiene exemption. The scanner still checks required physical columns, primary key shape, scope, indexes, constraints, and parse failures. If an existing registered system table misses a fixed-template requirement, the gate must report the difference first; schema repair belongs in a separate migration issue.

Metadata systems may manage display configuration, actions, views, and relation metadata for these tables, but must not add, delete, rename, or physically change registered system table columns.

## Owner-keyed operational tables

日志删除任务、固定删除工作集、停止事实和上传幂等回执按 application/job/event 身份查找；组织树及绑定按 workspace/parent/member 关系查找。它们不消费普通业务表的 scope chronology。`config.json` 只豁免不适用的具体字段/索引规则，`ownedTableContracts` 同时保留主键、非空 owner 外键与实际查询索引检查。合法树根允许 nullable `parent_id`；普通业务表仍要求 scope/time/id 索引。

原生消息投影进度复用 `flow_run_owned_table`：一个 run 一行、非空 flow_run 外键、主键索引，不能通过普通 exemption 掩盖丢失的 run owner。规则正例来自正式 migration inventory，受控反例移除主键、外键、owner 非空或查询索引时必须失败。

插件出站凭据按 `(publisher_namespace, plugin_code, scope_id, credential_id)` 精确查找，不虚构独立行 ID 或时间分页索引；仍检查 scope 与创建/更新生命周期字段。`plugin_schema_version_objects` 是 `(owner_id, owner_version, ownership_key)` 不可变版本成员集合，继续验证 ownership catalog 外键及 owner/version 查询前缀。两者使用同一 `ownedTableContracts`，不通过删门禁放行错误身份或丢失 owner。


通知配置与 inbox 是专用 owner-keyed operational 数据：实例按 workspace/created_at/id 查询，历史按 workspace/recipient/created_at/id 查询并通过 read_at 确认。只豁免不消费的通用 scope chronology 和不适用的 updated_at；`ownedTableContracts` 仍要求实例 workspace 外键、历史非空 workspace/recipient/instance 身份、read_at 与真实查询索引。历史特意不以用户/实例外键级联删除；没有 scope_id 不作为普通业务表 readiness 的替代，未将通知表伪装成 system/global 或 append-only。
