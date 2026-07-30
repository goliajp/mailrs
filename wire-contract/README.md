# wire-contract

Request bodies as the web client actually sends them, in one place, read by
both test suites.

## Why

The response direction is covered: Zod schemas, plus a rule requiring the
handler to be named and a real captured response used as the fixture
(`.claude/rules/frontend/wire-schema-verification.md`).

The request direction had nothing, and on 2026-07-30 an audit of all 35
request bodies found **nine wrong** — four failing every call, five
succeeding while dropping what the user had asked for (see
`.claude/notes/request-body-audit-2026-07-30.md`).

Tests existed and did not help. `api.test.ts` asserted the snooze body was
`{until: <ISO string>}` and passed on every run for months while every
snooze in production answered 422. It pinned what the frontend had decided
to send. **A test that checks one side against itself is shaped like a
contract test and verifies nothing.**

## How this is different

One file is the contract. Each side is checked against *it*, not against
itself:

- `crates/webapi/tests/request_contract.rs` deserializes each fixture into
  the struct the handler actually reads. A renamed or retyped field fails
  here.
- `web/src/wire/__tests__/request-contract.test.ts` calls the real endpoint
  function with a stubbed transport, captures the body it produced, and
  compares it to the fixture. A client-side change fails here.

Change either side without the other and one of the two goes red. Change
the fixture alone and both do.

## Adding one

1. Put the body in `requests/<name>.json`, exactly as the client sends it —
   copied from a captured request or from the endpoint function, not
   written from the struct.
2. Add a case to the Rust test naming the struct.
3. Add a case to the TS test naming the endpoint function.

The fixture is not a schema and does not need to cover optional fields. It
needs to be a body the client really produces.
