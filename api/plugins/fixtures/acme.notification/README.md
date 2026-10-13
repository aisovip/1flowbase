# Notification fixture

This test provider confirms receipts through the existing managed capability dispatcher. It uses separate instance configuration and send input, never contacts an external provider, and keeps credentials out of successful output.

Build `runtime-extension-sdk --example notification_worker`, then package this directory with `scripts/node/plugin/cli.js package --runtime-binary <absolute executable>`. The CLI supplies `bin/worker` from the manifest. Set `NOTIFICATION_WORKER_FIXTURE` to the executable for API integration tests. Missing binaries are test failures rather than silently skipped coverage.

Install and assign the package, grant `acme.notification-fixture.send` the workspace `notification.send` permission (`notification` / `1`), then enable it. The type appears in notification settings; save `channel` and password `token` as a notification instance and provide `text` per workflow send.

Inputs `fixture-invalid-output`, `fixture-secret-output`, and `fixture-provider-error` deliberately exercise Host rejection and redaction. They are fixture behaviors, not provider capabilities.

See [中文契约](../../../../docs/architecture/notifications.md) and [English contract](../../../../docs/architecture/notifications.en.md).
