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

// shell-level skeleton shown while the lazy Dashboard chunk is still
// downloading (before dashboard.tsx ever runs). matches the page's broad
// geometry so the swap from this → dashboard.tsx's own loading state is
// visually continuous instead of "spinner pops into a four-column grid".
// kept inline (not lazy) so it ships in the entry chunk and can paint
// at FCP.
function DashboardShellSkeleton() {
  const PulseBox = ({ className }: { className: string }) => (
    <div className={`bg-border animate-pulse rounded-lg ${className}`} />
  )
  return (
    <div className="h-full overflow-y-auto p-4 md:p-6">
      {/* greeting row + compose */}
      <div className="mb-6 flex items-start justify-between">
        <div className="space-y-2">
          <PulseBox className="h-6 w-56" />
          <PulseBox className="h-4 w-44" />
        </div>
        <div className="flex items-center gap-2">
          <PulseBox className="h-8 w-8 rounded-md" />
          <PulseBox className="h-8 w-24 rounded-md" />
        </div>
      </div>
      {/* search bar */}
      <PulseBox className="mb-6 h-10 w-full" />
      {/* 4 stat cards */}
      <div className="mb-6 grid grid-cols-2 gap-3 lg:grid-cols-4">
        {Array.from({ length: 4 }).map((_, i) => (
          <div
            className="border-border flex items-center gap-3 rounded-lg border px-4 py-3"
            key={i}
          >
            <div className="bg-border h-9 w-9 animate-pulse rounded-lg" />
            <div className="flex-1 space-y-1.5">
              <div className="bg-border h-6 w-10 animate-pulse rounded" />
              <div className="bg-border h-3 w-14 animate-pulse rounded" />
            </div>
          </div>
        ))}
      </div>
      {/* main grid: 2/3 left + 1/3 right */}
      <div className="grid gap-6 lg:grid-cols-3">
        <div className="space-y-6 lg:col-span-2">
          {/* big inbox status box */}
          <div className="border-border overflow-hidden rounded-lg border">
            <div className="border-border flex items-center justify-between border-b px-4 py-2.5">
              <div className="flex items-center gap-2">
                <PulseBox className="h-4 w-4" />
                <PulseBox className="h-4 w-20" />
              </div>
            </div>
            <div className="flex flex-col items-center gap-2 px-4 py-6">
              <PulseBox className="h-8 w-8 rounded-full" />
              <PulseBox className="mt-1 h-3 w-56" />
              <PulseBox className="mt-2 h-7 w-32 rounded-md" />
            </div>
          </div>
          {/* recent activity rows */}
          <div className="border-border overflow-hidden rounded-lg border">
            <div className="border-border flex items-center justify-between border-b px-4 py-2.5">
              <div className="flex items-center gap-2">
                <PulseBox className="h-4 w-4" />
                <PulseBox className="h-4 w-24" />
              </div>
              <PulseBox className="h-3 w-14" />
            </div>
            <div className="space-y-0.5 p-2">
              {Array.from({ length: 5 }).map((_, i) => (
                <div className="flex items-center gap-3 px-2 py-2" key={i}>
                  <div className="bg-border h-8 w-8 animate-pulse rounded-full" />
                  <div className="flex-1 space-y-1.5">
                    <PulseBox className="h-3.5 w-1/3" />
                    <PulseBox className="h-3 w-2/3" />
                  </div>
                  <PulseBox className="h-3 w-10" />
                </div>
              ))}
            </div>
          </div>
        </div>
        <div className="space-y-6">
          {/* categories with bars */}
          <div className="border-border overflow-hidden rounded-lg border">
            <div className="border-border flex items-center justify-between border-b px-4 py-2.5">
              <div className="flex items-center gap-2">
                <PulseBox className="h-4 w-4" />
                <PulseBox className="h-4 w-24" />
              </div>
            </div>
            <div className="space-y-2.5 p-3">
              {Array.from({ length: 6 }).map((_, i) => (
                <div className="space-y-1" key={i}>
                  <div className="flex items-center justify-between">
                    <PulseBox className="h-3 w-20" />
                    <PulseBox className="h-3 w-12" />
                  </div>
                  <div className="bg-bg-secondary h-1.5 overflow-hidden rounded-full">
                    <div
                      className="bg-border h-full animate-pulse rounded-full"
                      style={{ width: `${100 - i * 12}%` }}
                    />
                  </div>
                </div>
              ))}
            </div>
          </div>
          {/* top contacts */}
          <div className="border-border overflow-hidden rounded-lg border">
            <div className="border-border flex items-center justify-between border-b px-4 py-2.5">
              <div className="flex items-center gap-2">
                <PulseBox className="h-4 w-4" />
                <PulseBox className="h-4 w-24" />
              </div>
            </div>
            <div className="space-y-0.5 p-2">
              {Array.from({ length: 5 }).map((_, i) => (
                <div className="flex items-center gap-2.5 px-2 py-1.5" key={i}>
                  <div className="bg-border h-7 w-7 animate-pulse rounded-full" />
                  <div className="flex-1 space-y-1">
                    <PulseBox className="h-3.5 w-2/5" />
                    <PulseBox className="h-3 w-3/5" />
                  </div>
                  <PulseBox className="h-4 w-6 rounded-full" />
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
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
