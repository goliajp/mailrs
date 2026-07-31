# Response shapes, captured from production

The request direction has `../requests`. This is the other half: what the
backend actually sends, checked against what the client actually parses.

## Provenance

Every fixture's **key set, types and null-ness** were taken from a live
response on production, `t02.golia.jp`, on the date below. The **values are
synthetic**. That is deliberate: the fixture exists to pin the shape, and a
real capture would commit somebody's mail — subjects, addresses, snippets —
into the repository. Where a real value carries meaning the shape does not
(an enum member, a status string), the real one is used.

| fixture | endpoint | handler | captured |
|---|---|---|---|
| `conversation-list.json` | `GET /api/conversations` | `webapi/handlers/conversations.rs::get_conversations` → `Vec<ConversationResponse>` | 2026-07-31 |
| `conversation-categories.json` | `GET /api/conversations/categories` | `webapi/handlers/conversations.rs::get_categories` | 2026-07-31 |
| `send-list.json` | `GET /api/mail/sends` | `webapi/handlers/sends.rs` → the Send projection | 2026-07-31 |

To re-capture, with a live session token:

```bash
curl -s -H "Authorization: Bearer <token>" \
  http://localhost:3103/api/conversations?limit=1 | jq .
```

then replace the values, keeping every key, its type, and whether it was
`null`.

## Why this is not the same as the schema tests that already existed

`settings-schemas.test.ts` and friends assert that a hand-written object
parses. The object was written to match the schema, so it always parses —
the test restates the schema in a second syntax and cannot notice the
backend renaming a field. On 2026-07-30 nine request bodies were wrong and
every such test stayed green.

These fixtures are read by **both** sides:

- `crates/webapi/tests/response_contract.rs` serializes the handler's own
  response type and asserts its key set is the fixture's. A renamed or
  dropped field fails there.
- `web/src/wire/__tests__/response-contract.test.ts` parses the fixture with
  the Zod schema the client uses. A schema that has drifted fails there.

Neither side can pass by agreeing with itself.
