# Git Workflow

## Branching Model: git-flow (AVH)

Use **git-flow-avh** as the default branching model:

- `main` — production, always stable
- `develop` — integration branch
- `feature/*` — new features, branch from develop
- `bugfix/*` — bug fixes, branch from develop
- `release/*` — release prep, branch from develop
- `hotfix/*` — urgent production fixes, branch from main

```bash
git flow feature start <name>     # create feature branch
git flow feature finish <name>    # merge back to develop
git flow release start <version>  # cut a release
git flow release finish <version> # merge to main + develop, tag
git flow hotfix start <name>      # urgent fix from main
git flow hotfix finish <name>     # merge to main + develop, tag
```

## Commit Message Format

```
<type>: <description>

<optional body>
```

Types: feat, fix, refactor, docs, test, chore, perf, ci

Rules:
- All lowercase description, no trailing period
- No scope — write `feat:` not `feat(auth):`
- No AI co-author tags — never add Co-Authored-By
- Group related changes, commit dependencies first

## Pull Request Workflow

When creating PRs:
1. Analyze full commit history (not just latest commit)
2. Use `git diff [base-branch]...HEAD` to see all changes
3. Draft comprehensive PR summary
4. Include test plan with TODOs
5. Push with `-u` flag if new branch
6. PRs target `develop` by default, not `main`

## Feature Implementation Workflow

0. **Research & Reuse** — search GitHub (`gh search repos/code`), package registries, and open-source projects before writing anything new
1. **Plan** — break down into phases, identify dependencies and risks
2. **TDD** — write tests first (RED → GREEN → REFACTOR), verify 80%+ coverage
3. **Code Review** — address CRITICAL and HIGH issues before committing
4. **Commit & Push** — follow commit message format above
