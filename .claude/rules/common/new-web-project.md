# New Web Sub-Project

When the user asks to add a web app, frontend, admin panel, dashboard, or any React sub-project:

1. Copy the starter template:
   ```bash
   bunx degit goliajp/starters/web#develop <name>
   cd <name> && bun install
   ```

2. Clean up:
   - In `vite.config.ts`: set `base: '/'`, remove `__DEP_VERSIONS__` define
   - In `src/main.tsx`: remove `basename`, remove demo view imports, keep only a minimal home route
   - Delete demo files: `src/views/{home,components,state,about}.tsx`, `src/views/home.test.tsx`, `src/api/github.ts`
   - In `src/app.tsx`: update nav items and page title
   - In `package.json`: change `name` to match the project

3. Keep:
   - `src/api/client.ts` — change `baseURL` to the project's API
   - `src/components/theme-toggle.tsx`
   - `src/test-setup.ts`
   - `src/index.css`
   - All config files (tsconfig, eslint, prettier, vite)

4. Verify: `bun run check && bun run test && bun run build`
