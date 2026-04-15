# Coding Style

## Immutability (CRITICAL)

ALWAYS create new objects, NEVER mutate existing ones:

```
// Pseudocode
WRONG:  modify(original, field, value) → changes original in-place
CORRECT: update(original, field, value) → returns new copy with change
```

Rationale: Immutable data prevents hidden side effects, makes debugging easier, and enables safe concurrency.

## File Organization

MANY SMALL FILES > FEW LARGE FILES:
- High cohesion, low coupling
- 200-400 lines typical, 800 max
- Extract utilities from large modules
- Organize by feature/domain, not by type

## Null vs Zero (CRITICAL)

Null and zero are fundamentally different concepts. NEVER conflate them:

- `null` = unknown, not provided, not applicable → display nothing or "—"
- `0` / `00:00` / `""` = known value that happens to be zero/empty → display "0", "00:00", etc.

When converting between types (e.g., DATE → TIMESTAMPTZ), a zero time component (00:00) introduced by type casting is NOT real data — it is null. Do not display it.

```
WRONG:  time === "00:00" ? "00:00" : "—"   // treats cast artifact as data
RIGHT:  hasRealTime ? time : null           // only show genuinely recorded values
```

This applies everywhere: timestamps, amounts, counts, strings. If the source didn't provide the value, it is null regardless of what the storage layer fills in as a default.

## Error Handling

ALWAYS handle errors comprehensively:
- Handle errors explicitly at every level
- Provide user-friendly error messages in UI-facing code
- Log detailed error context on the server side
- Never silently swallow errors

## Input Validation

ALWAYS validate at system boundaries:
- Validate all user input before processing
- Use schema-based validation where available
- Fail fast with clear error messages
- Never trust external data (API responses, user input, file content)

## Comments

- Lowercase, no trailing period: `// handle edge case`
- Only when code isn't self-explanatory
- Never use decorative section markers like `--- xxx ---`
