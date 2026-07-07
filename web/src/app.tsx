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
import { Navigate, Route, Routes, useLocation } from 'react-router'

import { AppSidebar } from '@/components/app-sidebar'
import { CommandPalette } from '@/components/command-palette'
import { DashboardShellSkeleton } from '@/components/dashboard-skeleton'
import { MobileShell } from '@/components/mobile-shell'
import { MPane } from '@/layouts/pane'
import { Login } from '@/pages/login'
import { ResetPassword } from '@/pages/reset-password'
import { authAtom } from '@/store/auth'
import { connectionStatusAtom, unreadCountAtom } from '@/store/chat'

type HealthInfo = {
  kevy: boolean
  pg: boolean
  status: string
  version: string
}

const Admin = lazy(() => import('@/pages/admin').then((m) => ({ default: m.Admin })))
const Chat = lazy(() => import('@/pages/chat').then((m) => ({ default: m.Chat })))
const Dashboard = lazy(() => import('@/pages/dashboard').then((m) => ({ default: m.Dashboard })))
const Playground = lazy(() => import('@/pages/playground').then((m) => ({ default: m.Playground })))
const Protocol = lazy(() => import('@/pages/protocol').then((m) => ({ default: m.Protocol })))
const Settings = lazy(() => import('@/pages/settings').then((m) => ({ default: m.Settings })))

// apply zinc-neutral preset before first render — no effect race conditions
initMailrsTheme()

export function App() {
  useDocumentTitle()
  useThemeEffect()
  useFonts()
  useThemeColor()

  return (
    <>
      <ToastProvider position="top-right" />
      <CommandPalette />
      <Routes>
        <Route element={<Login />} path="/login" />
        <Route element={<ResetPassword />} path="/reset-password" />
        <Route
          element={
            <Suspense fallback={<LoadingFallback />}>
              <Playground />
            </Suspense>
          }
          path="/playground"
        />
        <Route
          element={
            <AuthShell>
              <PagePane>
                <Suspense fallback={<LoadingFallback />}>
                  <Protocol />
                </Suspense>
              </PagePane>
            </AuthShell>
          }
          path="/protocol"
        />
        <Route
          element={
            <AuthShell>
              <PagePane>
                <Suspense fallback={<LoadingFallback />}>
                  <Admin />
                </Suspense>
              </PagePane>
            </AuthShell>
          }
          path="/admin/*"
        />
        <Route
          element={
            <AuthShell>
              <PagePane>
                <Suspense fallback={<LoadingFallback />}>
                  <Settings />
                </Suspense>
              </PagePane>
            </AuthShell>
          }
          path="/settings"
        />
        <Route
          element={
            <AuthShell>
              <Suspense fallback={<LoadingFallback />}>
                <Chat />
              </Suspense>
            </AuthShell>
          }
          path="/mail/*"
        />
        <Route
          element={
            <AuthShell>
              <PagePane>
                <Suspense fallback={<DashboardShellSkeleton />}>
                  <Dashboard />
                </Suspense>
              </PagePane>
            </AuthShell>
          }
          path="/*"
        />
      </Routes>
    </>
  )
}

function AuthShell({ children }: { children: React.ReactNode }) {
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
          {children}
        </AppShell>
      </div>
      {/* mobile: independent shell with bottom nav */}
      <div className="contents md:hidden">
        <MobileShell>{children}</MobileShell>
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

function LoadingFallback() {
  return (
    <div className="text-fg-muted flex flex-1 flex-col items-center justify-center gap-3 p-8">
      <div className="border-border border-t-accent h-8 w-8 animate-spin rounded-full border-2" />
      <span className="text-sm">Loading…</span>
    </div>
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

function RequireAuth({ children }: { children: React.ReactNode }) {
  const auth = useAtomValue(authAtom)
  if (!auth) return <Navigate replace to="/login" />
  return children
}

declare const __WEB_VERSION__: string | undefined

function StatusBar() {
  const auth = useAtomValue(authAtom)
  const wsStatus = useAtomValue(connectionStatusAtom)
  const location = useLocation()
  const [health, setHealth] = useState<HealthInfo | null>(null)

  const fetchHealth = useCallback(async () => {
    const res = await fetch('/api/health')
    if (res.ok) setHealth(await res.json())
  }, [])

  useEffect(() => {
    void fetchHealth()
    const id = setInterval(fetchHealth, 30000)
    return () => clearInterval(id)
  }, [fetchHealth])

  const section = location.pathname.startsWith('/admin')
    ? 'Admin'
    : location.pathname.startsWith('/protocol')
      ? 'Monitor'
      : location.pathname.startsWith('/settings')
        ? 'Settings'
        : location.pathname.startsWith('/mail')
          ? 'Mail'
          : 'Home'

  const webVersion = typeof __WEB_VERSION__ !== 'undefined' ? __WEB_VERSION__ : 'dev'
  const backendOk = health?.status === 'healthy'
  const backendDot = backendOk
    ? 'bg-success'
    : health?.status === 'degraded'
      ? 'bg-warning'
      : health
        ? 'bg-danger'
        : 'bg-fg-muted'
  const backendLabel = health ? `Backend ${health.status}` : 'Backend contacting…'

  const wsDot =
    wsStatus === 'connected' ? 'bg-success' : wsStatus === 'connecting' ? 'bg-warning' : 'bg-danger'
  const wsLabel =
    wsStatus === 'connected' ? 'Live' : wsStatus === 'connecting' ? 'Connecting' : 'Offline'

  return (
    <div className="text-fg-secondary flex h-full items-center justify-between gap-4 px-4 text-xs">
      <div className="flex items-center gap-3">
        <span className="flex items-center gap-1.5" title={backendLabel}>
          <span className={`inline-block h-2.5 w-2.5 rounded-full ${backendDot}`} />
          <span>Backend</span>
        </span>
        <span className="text-border-strong">·</span>
        <span className="flex items-center gap-1.5" title={`WebSocket: ${wsStatus}`}>
          <span className={`inline-block h-2.5 w-2.5 rounded-full ${wsDot}`} />
          <span>Live · {wsLabel}</span>
        </span>
        {health && (
          <>
            <span className="text-border-strong">·</span>
            <span className="text-fg-muted">
              PG {health.pg ? '✓' : '✗'} · Kevy {health.kevy ? '✓' : '✗'}
            </span>
          </>
        )}
        <span className="text-border-strong">·</span>
        <span>{section}</span>
      </div>
      <div className="flex items-center gap-3">
        {auth && <span className="text-fg-muted">{auth.address}</span>}
        {auth && <span className="text-border-strong">·</span>}
        <span title={`webapp v${webVersion}${health ? ` · backend v${health.version}` : ''}`}>
          web v{webVersion}
          {health ? ` · api v${health.version}` : ''}
        </span>
      </div>
    </div>
  )
}

function useDocumentTitle() {
  const unreadCount = useAtomValue(unreadCountAtom)
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
