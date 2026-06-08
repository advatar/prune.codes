# API Deployment

`api.prune.codes` is served from `johansellstrom@192.168.2.126`.

## Services

- `codes.prune.sync`: launchd service for `prune-sync`
- `codes.prune.mcp`: launchd service for `prune-mcp`
- Service root: `/Users/johansellstrom/services/prune-api`
- Binaries: `/Users/johansellstrom/services/prune-api/bin`
- Data/index: `/Users/johansellstrom/services/prune-api/data`
- Logs: `/Users/johansellstrom/services/prune-api/logs`
- Secret env file: `/Users/johansellstrom/services/prune-api/prune-api.env`

`prune-mcp` listens on `0.0.0.0:47800` so the Colima Caddy container can reach it through `host.docker.internal:47800`. `prune-sync` listens on `0.0.0.0:47801` for the same reason.

## Public Routes

Caddy handles TLS and public routing:

- `GET /health` -> `prune-mcp`
- `GET /mcp` and `POST /mcp` -> `prune-mcp`
- `POST /github/webhook` -> `prune-sync`

`/sync` is intentionally not exposed publicly because it is a manual unauthenticated indexing trigger.

## Caddy

The public proxy is the `metabolog-caddy` container in the `coolify` Colima profile. Its Caddyfile is mounted from:

`/Users/johansellstrom/metabolog-token-broker/Caddyfile`

When replacing that file atomically, restart `metabolog-caddy` so Docker remounts the current inode:

```sh
ssh johansellstrom@192.168.2.126 'export PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin; colima ssh --profile coolify -- docker restart metabolog-caddy'
```

## Smoke Checks

```sh
curl -fsS https://api.prune.codes/health
curl -fsS https://api.prune.codes/mcp
curl -fsS -o /dev/null -w '%{http_code}\n' -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' https://api.prune.codes/mcp
```

Expected results:

- `/health` returns `{"status":"ok"}`
- `GET /mcp` returns `{"status":"ok"}`
- unauthenticated `POST /mcp` returns `401`

