import type React from 'react'

import {
  AppShell,
  themeAtom,
  themePresets,
  ToastProvider,
  useFonts,
  useResolvedMode,
  useThemeEffect,
} from '@goliapkg/gds'
import { useAtomValue } from 'jotai'
import { getDefaultStore } from 'jotai'
import { lazy, Suspense, useCallback, useEffect, useState } from 'react'
import { createBrowserRouter, Navigate, Outlet, RouterProvider, useLocation } from 'react-router'

import { AppSidebar } from '@/components/app-sidebar'
import { CommandPalette } from '@/components/command-palette'
import { DashboardShellSkeleton } from '@/components/dashboard-skeleton'
import { MobileShell } from '@/components/mobile-shell'
import { RouteErrorFallback } from '@/components/route-error-fallback'
import { type StatusBarHealth, StatusBarView } from '@/components/status-bar'
import { sectionForPath } from '@/components/status-bar-model'
import { useCurrentUnreadCount } from '@/hooks/use-current-mail-filters'
import { MPane } from '@/layouts/pane'
import { Login } from '@/pages/login'
import { ResetPassword } from '@/pages/reset-password'
import { authAtom } from '@/store/auth'
import { connectionStatusAtom } from '@/store/ui'

const Admin = lazy(() => import('@/pages/admin').then((m) => ({ default: m.Admin })))
const Chat = lazy(() => import('@/pages/chat').then((m) => ({ default: m.Chat })))
const Dashboard = lazy(() => import('@/pages/dashboard').then((m) => ({ default: m.Dashboard })))
const Playground = lazy(() => import('@/pages/playground').then((m) => ({ default: m.Playground })))
const Protocol = lazy(() => import('@/pages/protocol').then((m) => ({ default: m.Protocol })))
const Settings = lazy(() => import('@/pages/settings').then((m) => ({ default: m.Settings })))

// apply zinc-neutral preset before first render — no effect race conditions
initMailrsTheme()

// v2.1 phase-8: routes expressed as a data-router config
// (RFC decision #2). `createBrowserRouter` + `<RouterProvider>`
// replaces the old v6-style `<BrowserRouter><Routes>` tree. Route
// definitions move from JSX children in `<Routes>` to plain
// objects here — one entry per route, `element` is the rendered
// tree, `children` nests under layout routes.
// v2.1 §13.4 (2026-07-08): every route branch declares an
// `errorElement`, so a thrown wire error, render crash, or 401 in a
// child page renders the fallback instead of the browser default
// grey unstyled reload prompt.
const router = createBrowserRouter([
  {
    children: [
      { element: <Login />, errorElement: <RouteErrorFallback />, path: '/login' },
      {
        element: <ResetPassword />,
        errorElement: <RouteErrorFallback />,
        path: '/reset-password',
      },
      {
        element: (
          <Suspense fallback={<LoadingFallback />}>
            <Playground />
          </Suspense>
        ),
        errorElement: <RouteErrorFallback />,
        path: '/playground',
      },
      {
        children: [
          { element: <PageProtocol />, errorElement: <RouteErrorFallback />, path: '/protocol' },
          { element: <PageAdmin />, errorElement: <RouteErrorFallback />, path: '/admin/*' },
          { element: <PageSettings />, errorElement: <RouteErrorFallback />, path: '/settings' },
          { element: <PageChat />, errorElement: <RouteErrorFallback />, path: '/mail/*' },
          { element: <PageDashboard />, errorElement: <RouteErrorFallback />, path: '/*' },
        ],
        Component: AuthShellLayout,
        errorElement: <RouteErrorFallback />,
      },
    ],
    Component: RootLayout,
    errorElement: <RouteErrorFallback />,
  },
])

export function App() {
  return <RouterProvider router={router} />
}

/**
 * `AuthShell` as a route layout — checks auth, then renders the
 * shell (desktop `AppShell` + mobile `MobileShell`). Both call sites
 * used to receive `children` explicitly; here the child page comes
 * from `<Outlet />`.
 */
function AuthShellLayout() {
  return (
    <RequireAuth>
      {/* desktop: AppShell with sidebar + status bar */}
      <div className="hidden md:contents">
        <AppShell
          gap={6}
          padded
          sidebar={<AppSidebar />}
          sidebarWidth={56}
          statusBar={<StatusBar />}
        >
          <Outlet />
        </AppShell>
      </div>
      {/* mobile: independent shell with bottom nav */}
      <div className="contents md:hidden">
        <MobileShell>
          <Outlet />
        </MobileShell>
      </div>
    </RequireAuth>
  )
}

// apply zinc-neutral preset synchronously at module load time
// checks atom state (not localStorage) to ensure override fields are populated
function initMailrsTheme() {
  const store = getDefaultStore()
  const current = store.get(themeAtom)

  if (current.presetId === 'zinc-neutral' && current.colorOverridesDark !== null) {
    return
  }

  const preset = themePresets['zinc-neutral']
  store.set(themeAtom, {
    ...current,
    ...preset,
    colorOverrides: null,
    presetId: 'zinc-neutral',
  })
}

// Each `PageX` is `PagePane` + `Suspense` + the lazy-loaded page —
// hoisted out of the JSX-in-a-Route form so the router config stays
// declarative.

function LoadingFallback() {
  return (
    <div className="text-fg-muted flex flex-1 flex-col items-center justify-center gap-3 p-8">
      <div className="border-border border-t-accent h-8 w-8 animate-spin rounded-full border-2" />
      <span className="text-sm">Loading…</span>
    </div>
  )
}

function PageAdmin() {
  return (
    <PagePane>
      <Suspense fallback={<LoadingFallback />}>
        <Admin />
      </Suspense>
    </PagePane>
  )
}

function PageChat() {
  return (
    <Suspense fallback={<LoadingFallback />}>
      <Chat />
    </Suspense>
  )
}

function PageDashboard() {
  return (
    <PagePane>
      <Suspense fallback={<DashboardShellSkeleton />}>
        <Dashboard />
      </Suspense>
    </PagePane>
  )
}

function PagePane({ children }: { children: React.ReactNode }) {
  return (
    <>
      {/* desktop: MPane with padding and rounded corners */}
      <MPane className="hidden p-1 md:flex">{children}</MPane>
      {/* mobile: plain full-height container */}
      <div className="h-full overflow-y-auto md:hidden">{children}</div>
    </>
  )
}

function PageProtocol() {
  return (
    <PagePane>
      <Suspense fallback={<LoadingFallback />}>
        <Protocol />
      </Suspense>
    </PagePane>
  )
}

function PageSettings() {
  return (
    <PagePane>
      <Suspense fallback={<LoadingFallback />}>
        <Settings />
      </Suspense>
    </PagePane>
  )
}

function RequireAuth({ children }: { children: React.ReactNode }) {
  const auth = useAtomValue(authAtom)
  if (!auth) return <Navigate replace to="/login" />
  return children
}

/**
 * Root-layout route element. Owns the global side-effect hooks
 * (document title, theme, fonts, `<meta name="theme-color">`) plus
 * the persistent chrome (`ToastProvider`, `CommandPalette`) that
 * every route sees. `<Outlet />` renders the child route.
 */
function RootLayout() {
  useDocumentTitle()
  useThemeEffect()
  useFonts()
  useThemeColor()

  return (
    <>
      <ToastProvider position="top-right" />
      <CommandPalette />
      <Outlet />
    </>
  )
}

declare const __WEB_VERSION__: string | undefined

/**
 * Thin wrapper that reads live app state (auth atom, ws atom, react-router
 * location) and drives the pure {@link StatusBarView}. Everything ephemeral
 * lives here; anything visible lives in the presentational component.
 */
function StatusBar() {
  const auth = useAtomValue(authAtom)
  const wsStatus = useAtomValue(connectionStatusAtom)
  const location = useLocation()
  const [health, setHealth] = useState<null | StatusBarHealth>(null)

  const fetchHealth = useCallback(async () => {
    try {
      const res = await fetch('/api/health')
      if (res.ok) setHealth(await res.json())
    } catch {
      // The status bar treats fetch errors as "still contacting" — the
      // view renders a neutral dot rather than a red one on transient
      // network hiccups.
    }
  }, [])

  useEffect(() => {
    void fetchHealth()
    const id = setInterval(fetchHealth, 30000)
    return () => clearInterval(id)
  }, [fetchHealth])

  const webVersion =
    typeof __WEB_VERSION__ !== 'undefined' && __WEB_VERSION__ !== '0.0.0' ? __WEB_VERSION__ : 'dev'

  return (
    <StatusBarView
      backend={health}
      identity={auth?.address}
      realtime={wsStatus as never}
      section={sectionForPath(location.pathname)}
      webVersion={webVersion}
    />
  )
}

function useDocumentTitle() {
  const unreadCount = useCurrentUnreadCount()
  useEffect(() => {
    document.title = unreadCount > 0 ? `(${unreadCount}) Mailrs` : 'Mailrs'
  }, [unreadCount])
}

// sync <meta name="theme-color"> with dark/light mode
function useThemeColor() {
  const mode = useResolvedMode()
  useEffect(() => {
    const meta = document.querySelector('meta[name="theme-color"]')
    if (meta) {
      meta.setAttribute('content', mode === 'dark' ? '#09090b' : '#dc2626')
    }
  }, [mode])
}
