# Single-file Docker deployment

[简体中文](README_CN.md)

Save `compose.yaml` in an empty directory and run:

```bash
docker compose up -d
```

Open `http://localhost:3100`. The initial account is `root` and the initial password is
`change-me-root-password`; change it after signing in. This recipe requires an API
image containing `/usr/local/bin/1flowbase-initialize` and
`/usr/local/bin/1flowbase-api-start`. Images published before this recipe do not contain them.

The initialization service runs a POSIX shell script built into the API image without
using Node.js. It creates mounted
directories and `config/.env`, generates a separate database password and encryption
master key, then exits. PostgreSQL and the API read the saved configuration.
Recreating containers preserves it. Keep the configuration together with the database
and uploaded files; an existing database without its configuration fails to start.

Before first startup, an optional Compose `.env` next to `compose.yaml` can set
`WEB_PORT`, `FLOWBASE_API_SERVER_VERSION`, `FLOWBASE_WEB_VERSION`,
`BOOTSTRAP_ROOT_ACCOUNT`, `BOOTSTRAP_ROOT_PASSWORD`, `POSTGRES_PASSWORD`, and
`API_PROVIDER_SECRET_MASTER_KEY`. Initialization values apply only when creating
`config/.env`; changing them does not reset existing accounts or rotate existing secrets.
The default is HTTP. Set `API_COOKIE_SECURE=true` for a browser-facing HTTPS entry,
including TLS terminated by an outer nginx proxy. Set `API_ALLOWED_ORIGINS` when
browser clients access the API from a different origin.

`config/.env` contains secrets. Do not commit it or delete it to reset an account.
This recipe is for a fresh deployment with bundled PostgreSQL. Existing deployments,
external PostgreSQL, and portable recovery retain the deployment scripts and recipes
documented in [the Docker guide](../README.md).

## Automatic domestic registry mirroring

`aliyun image sync` is a separate GitHub Actions workflow. It runs after a successful
`container images` workflow on the `latest` branch. A mirror failure does not change
the GHCR or GitHub Release result. The new mirror workflow and its script must be
present on the repository's default branch for `workflow_run` to trigger automatically.

Configure Settings → Secrets and variables → Actions:

| Name | Type | Value |
| --- | --- | --- |
| `ACR_SYNC_ENABLED` | Variable | `true` to enable; any other value disables syncing |
| `ACR_REGISTRY` | Variable | Public registry hostname, without `https://` |
| `ACR_REPOSITORY` | Variable | `namespace/repository` |
| `ACR_USERNAME` | Variable | Registry login username |
| `ACR_PASSWORD` | Secret | Registry login password |

The mirror processes only components successfully published by the source run. Each
component uses its own version, and its image revision must match the publishing
commit. API and Web share the target repository under `api-v<version>`,
`web-v<version>`, `api-latest`, and `web-latest`. Copying preserves runtime images
and per-platform image digests for `linux/amd64` and `linux/arm64` without rebuilding.
Personal ACR does not support OCI build attestations, so they are excluded from the
destination index. Its top-level digest changes; verification compares both runtime
image digests individually. The copier is regctl v0.11.6, pinned by version and checksum,
used only in CI without adding application dependencies.
A component's domestic latest tag is updated
only when GHCR latest still points to that image.

Retry a failed mirror run, or select Run workflow and provide the successful source
`container images` run ID from this repository's `latest` branch. Syncing fails if a
candidate was not promoted to an official version, the source image was overwritten
by another commit, or runtime image digests cannot be preserved. Verification evidence is saved
in the mirror run's `aliyun-image-sync-*` artifact.

To deploy from the mirror, set both API and init `image` values in Compose to
`hostname/namespace/repository:api-v<version>`, and Web to
`hostname/namespace/repository:web-v<version>`. This workflow does not edit Compose.
For a private registry, log in on the deployment machine before starting the services.
