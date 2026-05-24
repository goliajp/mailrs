# v0.5 API Drift Audit

> Server Refactor v2 checkpoint v0.5。
> 比对 4 份 API 声明的一致性：REST router、MCP code、OpenAPI spec、
> llm-full.txt。

## 范围

| 维度 | 来源 | 总数 |
|---|---|---|
| REST router | `crates/server/src/web/**/*.rs` `.route()`/`.nest()` callsites | **148** |
| MCP tools (code) | `crates/server/src/mcp/mod.rs` 内 `#[tool]` 修饰的 fn | **58** |
| MCP tools (doc) | `web/public/llm-full.txt` 内 tool 表 | 52 |
| OpenAPI paths | `web/public/openapi.json` | **98** |

## Drift 矩阵

### 1. MCP code vs llm-full.txt doc

| 方向 | 数 | 影响 |
|---|---|---|
| code 有 doc 缺 | **6** | LLM 调用者看不到这些 tool |
| doc 有 code 缺 | 0 | — |

**Code 但 doc 缺的 6 个 tools:**

- `audit_list_conversations`
- `audit_read_thread`
- `get_info`
- `get_system_config`
- `reset_system_config`
- `set_system_config`

### 2. REST router vs OpenAPI

| 方向 | 数 | 影响 |
|---|---|---|
| router 有 openapi 缺 | **50** | OpenAPI client (autodiscover、generated SDK) 看不到 |
| openapi 有 router 缺 | 0 | — |

**50 个缺失 endpoint 按子系统分类:**

**OIDC + OAuth (8)**
- `/.well-known/jwks.json`
- `/.well-known/openid-configuration`
- `/oauth/authorize`
- `/oauth/token`
- `/oauth/userinfo`
- `/api/auth/oidc/callback`
- `/api/auth/oidc/config`
- `/api/auth/oidc/login`

**Admin (12)**
- `/api/admin/audit/accounts`
- `/api/admin/audit/conversations`
- `/api/admin/audit/conversations/{thread_id}/messages`
- `/api/admin/audit/messages/{uid}/raw`
- `/api/admin/export`
- `/api/admin/oauth-clients`
- `/api/admin/oauth-clients/{client_id}`
- `/api/admin/rbl-status`
- `/api/admin/reputation`
- `/api/admin/spam-feedback-stats`
- `/api/admin/suppressions`
- `/api/admin/system-config` + `/api/admin/system-config/{key}`

**Auth (4)**
- `/api/auth/change-password`
- `/api/auth/recovery-email`
- `/api/auth/verify`
- `/api/auth/verify-totp`

**Mail (7)**
- `/api/mail/ai/generate-subject`
- `/api/mail/check-deliverability`
- `/api/mail/render-preview`
- `/api/mail/render-preview/cache/{id}`
- `/api/mail/spam-feedback`
- `/api/mail/stats`
- `/api/bimi/{domain}`

**Conversations (2)**
- `/api/conversations/action-count`
- `/api/conversations/{thread_id}/dismiss-action`

**Calendar / Invites (5)**
- `/api/calendar/conflicts`
- `/api/calendar/feeds`
- `/api/calendar/feeds/{feed_id}`
- `/api/invites/{message_id}/counter`
- `/api/invites/{message_id}/rsvp`

**DAV (5)** — CalDAV/CardDAV roots; protocol clients use these
- `/dav/`
- `/dav/calendars/{user}/`
- `/dav/calendars/{user}/{calendar}/`
- `/dav/contacts/{user}/`
- `/dav/contacts/{user}/{book}/`

**Proxy (2)**
- `/api/proxy/image`
- `/api/proxy/link`

**Misc (5)**
- `/api/events` (WebSocket)
- `/jmap/eventsource/` (Server-Sent Events)
- `/Autodiscover/Autodiscover.xml` (Outlook autodiscover)
- `/mail/config-v1.1.xml` (Thunderbird autoconfig)

## 修复策略（优先级 + 估算）

按对外部 client 影响排序：

| 批次 | 子系统 | endpoint 数 | 理由 |
|---|---|---|---|
| 1 | OIDC + OAuth | 8 | OAuth 客户端 (3rd party app integration) 必看 OpenAPI |
| 2 | Admin | 12 | Admin SDK / Terraform-style 自动化用 |
| 3 | Auth | 4 | 用户管理流程的"修密码/验证 email"等 |
| 4 | Mail extras | 7 | mailing UI 用 |
| 5 | Conversations | 2 | mailing UI 用 |
| 6 | Calendar / Invites | 5 | RSVP / iTIP 用 |
| 7 | DAV | 5 | CalDAV/CardDAV 客户端用（不一定需要 OpenAPI 但加上利大于弊） |
| 8 | Proxy | 2 | 内部 |
| 9 | Misc | 5 | Autodiscover 不是 REST，可标 deprecated/non-rest |
| 10 | MCP doc sync | 6 | 在 llm-full.txt 加 6 行 + 调 tool count |

**当前 audit 一轮过完后**，加 CI lint `scripts/check-api-drift.sh` 比对
router vs openapi，防回流。

## CI lint script (草案)

```bash
#!/usr/bin/env bash
# Forbid API drift between REST router and openapi.json.
# Pre-flight: run before release.sh; fails if any router endpoint
# is missing from openapi.json or vice versa.
set -euo pipefail
python3 - <<'PY'
import json, os, re
routes = set()
for root, _, files in os.walk('crates/server/src'):
    for f in files:
        if not f.endswith('.rs'): continue
        text = open(os.path.join(root, f)).read()
        for m in re.finditer(r'\.route\s*\(\s*"([^"]+)"', text):
            routes.add(m.group(1))
api = set(json.load(open('web/public/openapi.json'))['paths'].keys())
missing_in_api = sorted(routes - api)
extra_in_api = sorted(api - routes)
if missing_in_api or extra_in_api:
    if missing_in_api:
        print('FAIL: router has endpoints missing from openapi.json:')
        for p in missing_in_api: print(' ', p)
    if extra_in_api:
        print('FAIL: openapi.json has phantom endpoints:')
        for p in extra_in_api: print(' ', p)
    raise SystemExit(1)
print(f'OK: router and openapi.json in sync ({len(routes)} endpoints)')
PY
```
