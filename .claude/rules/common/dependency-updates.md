# Dependency Updates

## Principle

Maintain a strong bias toward latest stable versions. Outdated dependencies accumulate security debt and compatibility friction. However, upgrades must be deliberate, not blind.

## Workflow

1. **Check** — run `bunx npm-check-updates` (or `ncu`) to see what's available
2. **Analyze** — for each outdated dependency, classify the upgrade:
   - **patch/minor** (e.g., 2.1.0 → 2.1.3, 2.1.0 → 2.3.0) — safe, apply with `ncu -u --target minor`
   - **major** (e.g., 4.x → 5.x) — check changelog/migration guide, report to user before applying
3. **Discuss** — present major upgrades to the user with: package name, current → target version, breaking changes summary, migration effort estimate
4. **Apply** — after user approval, upgrade and verify: `bun install && bun run check && bun run test && bun run build`
5. **Never** run `ncu -u` (unlimited) without review — it upgrades across major versions silently

## When to Check

- Before any release or deployment
- When adding a new dependency (ensure existing deps are not stale)
- When a build or test failure hints at version incompatibility
- Periodically during development sessions — suggest proactively if deps are significantly behind

## Risk Assessment

| Signal | Action |
|--------|--------|
| Patch available | Apply without asking |
| Minor available | Apply, mention in commit message |
| Major available | Stop and discuss with user |
| Pre-release / alpha / rc | Ignore unless user explicitly requests |
| Dependency deprecated | Flag immediately, find replacement |

## Post-Upgrade Verification

After any upgrade, the full pipeline must pass before committing:

```bash
bun install
bun run check    # tsc + eslint + prettier
bun run test     # vitest
bun run build    # production build
```

If any step fails, diagnose and fix before proceeding — do not commit broken upgrades.
