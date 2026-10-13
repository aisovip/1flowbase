# 通知实例与插件发送

通知有三个独立对象：类型描述、工作区通知实例、用户站内消息。设置页保存实例配置；工作流通知节点选择实例并提供本次输入。底座只内置 `system` 类别，其他类别由受管插件贡献。

## 数据与结果

`notification_instances` 保存 `workspace_id`、`type_code`、`installation_id`、`display_name`、`status`、公开 `config_json` 和加密秘密配置。密码控件字段由宿主加密，查询只返回 `configured_secret_fields` 字段名。更新时省略密码字段会保留已有值。

`system_notifications` 按收件用户保存独立消息及 `read_at`。发送必须指定当前工作区有效成员，所有收件人验证成功后事务提交，随后发布刷新信号。消息列表、未读数和标已读都以当前工作区、当前用户过滤。删除实例、关闭或卸载插件保留已有消息；实例不存在、停用或插件不可用时发送明确失败。

站内通知成功表示消息事务已提交；外部通知结果由插件对供应商的接收确认定义。成功不表示用户已读。`notification.changed` SSE 只通知客户端回读 HTTP 快照；断线重连也回读数据库。刷新信号不是持久消息，不承诺客户端收到每个事件。

## 插件描述

完整示例见 [notification fixture](../../api/plugins/fixtures/acme.notification/manifest.yaml)。`notification_types` 声明以下字段：

| 字段 | 职责 |
| --- | --- |
| `type_code` | `publisher_namespace/plugin_id/local_type`，`system` 保留给宿主 |
| `config_schema` / `config_form` | 保存到实例的 JSON Schema 与既有 `PluginFormSchema`（`schema_version: 1.0.0`） |
| `input_schema` / `input_form` | 单次发送的参数与表单，与配置分离 |
| `output_schema` | 单次结果对象的 JSON Schema |
| `contribution_id` | 对应受管发送贡献 |

表单使用已有 string、number、boolean、select 控件、默认值与条件；`control: password` 只用于配置中的 string 字段，`control: multi-select` 对应 array schema，选项值按 `items` 校验。Schema 自包含，不解析外部 `$ref`。系统类型的收件人选项由类型 API 按当前工作区成员提供。

贡献绑定到 `1flowbase.notification.send`、契约 `1flowbase.notification/v1`，请求 `notification.send` 权限，并声明 `payload: {kind: notification_type, type_code: ...}`。安装、工作区分配、贡献授权和启用沿现有插件生命周期；manifest 声明不等于授权。贡献授权使用 `permission_contract_id: notification`、`permission_contract_version: 1`、workspace resource scope。

## 实现发送

工作进程沿现有 capability envelope 接收 `method: execute` 与 `input.config_payload`、`input.input_payload`。共享 Host 负责实例校验、可信 actor/workspace、有效贡献快照、独立授权、进程分发与生命周期。插件负责供应商发送和输出，不维护第二套宿主通信或数据库连接。

Rust 作者可以复用 `runtime_extension_sdk::execute_notification`；stdio one-shot 示例见 [SDK worker](../../api/crates/runtime-extension-sdk/src/_tests/notification_worker.rs)。multiplex 插件在已有 `serve` handler 中反序列化 `CapabilityStdioRequest`，调用同一 helper 后序列化响应即可。首版支持 `process_per_call` 的 stdio 与 multiplex，不声明 notification 的 actor service pool。

```bash
cargo build --locked --manifest-path api/Cargo.toml -p runtime-extension-sdk --example notification_worker
export NOTIFICATION_WORKER_FIXTURE="$(pwd)/api/target/debug/examples/notification_worker"
node scripts/node/plugin/cli.js package api/plugins/fixtures/acme.notification \
  --out tmp/notification-packages --runtime-binary "$NOTIFICATION_WORKER_FIXTURE" \
  --target x86_64-unknown-linux-gnu
```

该 fixture 不联系外部服务。实际构建输出位置应沿本机 Cargo target 配置解析；上面的默认路径不是已执行证明。主仓不修改或发布 SSH/OpenAI 插件。

## 工作流和 API

节点 `type: notification` 保存 `config.instance_id`。`bindings.input` 使用既有 `named_bindings`，每字段沿描述中的 key，支持 constant、selector 和 templated_text；数字表达式沿 `valueType: number`。例如：

```json
{"config":{"instance_id":"00000000-0000-0000-0000-000000000000"},"bindings":{"input":{"kind":"named_bindings","value":[{"name":"text","valueType":"string","value":{"kind":"templated_text","value":"任务 {{ upstream.result }} 已完成"}}]}}}
```

发送输出为 `{instance_id, type_code, result}`。默认 `result` 输出变量使用选中类型的 `output_schema`；下游可选择 `result` 内的字段。已发布/恢复执行使用冻结的工作流 actor/workspace，不重新套用用户后来切换的工作区。调试单节点只发送本节点，不重放上游。

HTTP owner 是 `/api/console/notifications`：types、instances CRUD、`instances/{id}/send`、messages、unread-count、read、read-all 和 events。设置操作沿独立 console operations；本人消息与类型读取使用认证入口。业务 HTTP 经 Canonical Interface/typed Handler；events 是只读刷新协议控制入口。API、工作流复用同一个 `NotificationService`。

测试以实际 package intake、PostgreSQL、独立授权与 SDK 二进制覆盖发送、schema 拒绝、秘密脱敏和不可用实例保留；测试路径与执行证据记录在当前任务的 `tmp/test-governance/`，源码或复现命令本身不代表测试已通过。
