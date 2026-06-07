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
import { ErrorBoundary } from '@/components/error-boundary'
import { MobileShell } from '@/components/mobile-shell'
import { MPane } from '@/layouts/pane'
import { Login } from '@/pages/login'
import { ResetPassword } from '@/pages/reset-password'
import { authAtom } from '@/store/auth'
import { connectionStatusAtom, unreadCountAtom } from '@/store/chat'

// every authenticated page is lazy so the entry chunk is just the shell +
// auth gate. cuts cold-load JS preload by ~875 KB on /login (perfs/topic-03).
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
    <ErrorBoundary>
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
                {/* dashboard-shaped fallback during the lazy chunk fetch
                    so the user doesn't see a generic spinner give way to
                    a structured page — the shell stays the whole time */}
                <Suspense fallback={<DashboardShellSkeleton />}>
                  <Dashboard />
                </Suspense>
              </PagePane>
            </AuthShell>
          }
          path="/*"
        />
      </Routes>
    </ErrorBoundary>
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
    <div className="flex flex-1 items-center justify-center p-8">
      <div className="border-border border-t-accent h-5 w-5 animate-spin rounded-full border-2" />
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

function StatusBar() {
  const auth = useAtomValue(authAtom)
  const wsStatus = useAtomValue(connectionStatusAtom)
  const location = useLocation()
  const [health, setHealth] = useState<null | {
    kevy: boolean
    pg: boolean
    status: string
    version: string
  }>(null)

  const fetchHealth = useCallback(async () => {
    try {
      const res = await fetch('/api/health')
      if (res.ok) setHealth(await res.json())
    } catch {
      /* ignore */
    }
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

  return (
    <div className="text-fg-muted flex h-full items-center justify-between px-3 text-[11px]">
      <div className="flex items-center gap-2">
        {health && (
          <span className="flex items-center gap-1">
            <span
              className={`inline-block h-2 w-2 rounded-full ${health.status === 'healthy' ? 'bg-success' : health.status === 'degraded' ? 'bg-warning' : 'bg-danger'}`}
            />
            {health.pg ? 'PG' : ''}
            {health.pg && health.kevy ? ' · ' : ''}
            {health.kevy ? 'Kevy' : ''}
          </span>
        )}
        <span className="flex items-center gap-1">
          <span
            className={`inline-block h-2 w-2 rounded-full ${wsStatus === 'connected' ? 'bg-success' : wsStatus === 'connecting' ? 'bg-warning' : 'bg-danger'}`}
            title={`WS: ${wsStatus}`}
          />
          <span className="hidden sm:inline">{wsStatus === 'offline' ? 'Offline' : ''}</span>
        </span>
        <span className="text-border-strong">·</span>
        <span>{section}</span>
      </div>
      <div className="flex items-center gap-2">
        {auth && <span>{auth.address}</span>}
        {auth && health && <span className="text-border-strong">·</span>}
        {health && <span>v{health.version}</span>}
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
