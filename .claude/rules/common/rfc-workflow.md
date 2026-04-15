# RFC Workflow

For non-trivial changes, write an RFC before implementation:

0. **Research first** — search GitHub (`gh search repos/code`), package registries, and existing open-source projects before inventing anything new. If a solution already exists, reuse or adapt it rather than reimplementing.
1. Enter plan mode
2. Create `.claude/rfcs/YYYYMMDD-<slug>.md`
3. Include: context, approach, affected files, test cases, risks
4. Get approval before implementing
5. Reference RFC in commit messages
