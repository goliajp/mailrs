import type React from 'react'

import {
  AppShell,
  ToastProvider,
  useFonts,
  useSetThemePreset,
  useThemeEffect,
} from '@goliapkg/gds'
import { useAtomValue } from 'jotai'
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react'
import { Navigate, Route, Routes, useLocation } from 'react-router'

import { AppSidebar } from '@/components/app-sidebar'
import { CommandPalette } from '@/components/command-palette'
import { ErrorBoundary } from '@/components/error-boundary'
import { MPane } from '@/layouts/pane'
import { Chat } from '@/pages/chat'
import { Dashboard } from '@/pages/dashboard'
import { Login } from '@/pages/login'
import { ResetPassword } from '@/pages/reset-password'
import { authAtom } from '@/store/auth'
import { unreadCountAtom } from '@/store/chat'

const Admin = lazy(() =>
  import('@/pages/admin').then((m) => ({ default: m.Admin }))
)
const Playground = lazy(() =>
  import('@/pages/playground').then((m) => ({ default: m.Playground }))
)
const Protocol = lazy(() =>
  import('@/pages/protocol').then((m) => ({ default: m.Protocol }))
)
const Settings = lazy(() =>
  import('@/pages/settings').then((m) => ({ default: m.Settings }))
)

export function App() {
  useDocumentTitle()
  useMailrsTheme()
  useThemeEffect()
  useFonts()

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
              <Chat />
            </AuthShell>
          }
          path="/mail/*"
        />
        <Route
          element={
            <AuthShell>
              <PagePane>
                <Dashboard />
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
      <AppShell
        gap={6}
        padded
        sidebar={<AppSidebar />}
        sidebarWidth={56}
        statusBar={<StatusBar />}
      >
        {children}
      </AppShell>
    </RequireAuth>
  )
}

function LoadingFallback() {
  return (
    <div className="flex flex-1 items-center justify-center p-8">
      <div className="border-border border-t-accent h-5 w-5 animate-spin rounded-full border-2" />
    </div>
  )
}

function PagePane({ children }: { children: React.ReactNode }) {
  return <MPane className="p-1">{children}</MPane>
}

function RequireAuth({ children }: { children: React.ReactNode }) {
  const auth = useAtomValue(authAtom)
  if (!auth) return <Navigate replace to="/login" />
  return children
}

function StatusBar() {
  const auth = useAtomValue(authAtom)
  const location = useLocation()
  const [health, setHealth] = useState<null | {
    pg: boolean
    status: string
    valkey: boolean
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
            {health.pg && health.valkey ? ' · ' : ''}
            {health.valkey ? 'Valkey' : ''}
          </span>
        )}
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

// ensure zinc-neutral preset is active (one-time migration + default for new users)
function useMailrsTheme() {
  const setPreset = useSetThemePreset()
  const initialized = useRef(false)

  useEffect(() => {
    if (initialized.current) return
    initialized.current = true
    try {
      const stored = localStorage.getItem('gds-theme')
      const parsed = stored ? JSON.parse(stored) : null
      if (!parsed || parsed.presetId !== 'zinc-neutral') {
        setPreset('zinc-neutral')
      }
    } catch {
      setPreset('zinc-neutral')
    }
  }, [setPreset])
}
