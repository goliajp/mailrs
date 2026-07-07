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
import { type StatusBarHealth, StatusBarView } from '@/components/status-bar'
import { sectionForPath } from '@/components/status-bar-model'
import { useCurrentUnreadCount } from '@/hooks/use-current-mail-filters'
import { MPane } from '@/layouts/pane'
import { Login } from '@/pages/login'
import { ResetPassword } from '@/pages/reset-password'
import { authAtom } from '@/store/auth'
import { connectionStatusAtom } from '@/store/chat'

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
