import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useEffect, useRef } from 'react'
import { useSearchParams } from 'react-router'

import { ConversationList } from '@/components/conversation-list'
import { DraftsList } from '@/components/drafts-list'
import { KeyboardShortcutsDialog } from '@/components/keyboard-shortcuts-dialog'
import { MobileMail } from '@/components/mobile-mail'
import { NewConversation } from '@/components/new-conversation'
import { SendList } from '@/components/send-list/send-list'
import { ThreadView } from '@/components/thread-view'
import { useCurrentSelection } from '@/hooks/use-current-list'
import { useKeyboardNav } from '@/hooks/use-keyboard-nav'
import { useMailEvents } from '@/hooks/use-mail-events'
import { MPane, MPaneGroup } from '@/layouts/pane'
import { isMailListId, MAIL_LISTS, type MailListId } from '@/lib/mail-lists'
import { authAtom } from '@/store/auth'
import {
  activeListAtom,
  categoryFilterAtom,
  composingNewAtom,
  importanceSectionAtom,
  mobileViewAtom,
  openThreadAtom,
  selectMailListAtom,
  shortcutsDialogOpenAtom,
} from '@/store/ui'

export function Chat() {
  const auth = useAtomValue(authAtom)
  const composingNew = useAtomValue(composingNewAtom)
  const activeList = useAtomValue(activeListAtom)
  const selectList = useSetAtom(selectMailListAtom)
  const openThread = useSetAtom(openThreadAtom)
  const [mobileView, setMobileView] = useAtom(mobileViewAtom)
  const [shortcutsOpen, setShortcutsOpen] = useAtom(shortcutsDialogOpenAtom)
  const [importanceSection, setImportanceSection] = useAtom(importanceSectionAtom)
  const [categoryFilter, setCategoryFilter] = useAtom(categoryFilterAtom)
  const selection = useCurrentSelection()
  const [searchParams, setSearchParams] = useSearchParams()

  // Single effect that owns the URL <-> state sync:
  //   - first run: restore from URL params (and skip writing back, so we
  //     don't clobber the URL before those writes flush)
  //   - subsequent runs: write state into the URL
  // Keeping it in one effect avoids the race between a separate "restore"
  // and "sync" pair where the sync's first invocation captures the
  // defaults and overwrites the URL to empty before the restore lands.
  //
  // `?list=` replaced `?folder=` + `?tab=`, which between them encoded
  // the same thing twice. The old pair is still read, because somebody
  // has a tab open on one.
  const initializedRef = useRef(false)
  useEffect(() => {
    if (!initializedRef.current) {
      initializedRef.current = true
      const urlThread = searchParams.get('thread')
      const urlMsg = searchParams.get('msg')
      const urlView = searchParams.get('view') as
        | 'conversation'
        | 'list'
        | 'reply'
        | 'thread'
        | null
      const urlCat = searchParams.get('cat')
      const restored = listFromParams(searchParams)
      if (restored) selectList(restored)
      if (urlView) setMobileView(urlView)
      if (urlCat) setCategoryFilter(urlCat)
      const tab = searchParams.get('tab')
      if (tab === 'important' || tab === 'other') setImportanceSection(tab)
      // Last, and with the list named: `selectList` clears the pick, so
      // restoring the thread before it would throw the thread away.
      if (urlThread) {
        openThread({
          list: restored ?? 'inbox',
          threadId: urlThread,
          uid: urlMsg ? Number(urlMsg) : null,
        })
      }
      return
    }
    const params = new URLSearchParams()
    if (selection) params.set('thread', selection.threadId)
    if (selection?.uid != null) params.set('msg', String(selection.uid))
    if (mobileView !== 'list') params.set('view', mobileView)
    params.set('list', activeList)
    if (importanceSection) params.set('tab', importanceSection)
    if (categoryFilter) params.set('cat', categoryFilter)
    const newSearch = params.toString()
    if (newSearch !== searchParams.toString()) {
      setSearchParams(params, { replace: true })
    }
  }, [
    selection,
    mobileView,
    activeList,
    importanceSection,
    categoryFilter,
    searchParams,
    setSearchParams,
    selectList,
    openThread,
    setMobileView,
    setImportanceSection,
    setCategoryFilter,
  ])

  useEffect(() => {
    // iOS Safari has no Notification global outside installed PWAs —
    // a bare reference throws ReferenceError inside this effect, which
    // unmounts the whole tree (the mobile inbox white-screen incident).
    if ('Notification' in window && Notification.permission === 'default') {
      void Notification.requestPermission()
    }
  }, [])

  // websocket events
  useMailEvents(auth?.address ?? '')

  // keyboard navigation
  useKeyboardNav()

  // The conversation query lived here as well as in the list, with its
  // own copy of the filter memo and an effect that selected its first
  // row — even while Send or Draft was the list on screen. Both are
  // gone: `ConversationList` owns the query for the list it draws, and
  // the selection is derived from whichever list that is.
  // Which list component to draw, off the same value everything else
  // reads — one switch over the source kind rather than a chain of
  // folder comparisons that had to agree with the tab resolver.
  const renderList = () => {
    switch (MAIL_LISTS[activeList].source.kind) {
      case 'drafts':
        return <DraftsList />
      case 'sends':
        return <SendList />
      case 'threads':
        return <ConversationList onSelectConversation={() => setMobileView('thread')} />
    }
  }

  const renderMobileBody = () => {
    if (mobileView === 'list') return renderList()
    return <MobileMail />
  }

  const renderDesktopMain = () => {
    if (composingNew) {
      return (
        <MPane>
          <NewConversation />
        </MPane>
      )
    }
    return <ThreadView onBack={() => setMobileView('list')} />
  }

  return (
    <>
      {/* ─── MOBILE: full-screen view switching ─── */}
      <div className="h-full md:hidden">{renderMobileBody()}</div>

      {/* ─── DESKTOP: side-by-side pane layout ─── */}
      <MPaneGroup className="hidden md:flex">
        <MPane width={480}>{renderList()}</MPane>

        <MPaneGroup>{renderDesktopMain()}</MPaneGroup>

        <KeyboardShortcutsDialog onClose={() => setShortcutsOpen(false)} open={shortcutsOpen} />
      </MPaneGroup>
    </>
  )
}

/**
 * The list a URL names.
 *
 * `?list=` is the current spelling. `?folder=` + `?tab=` was the old
 * pair — two params that between them encoded one fact, and could
 * disagree — and it is still read so a tab somebody left open on it
 * lands where it did before.
 */
function listFromParams(params: URLSearchParams): MailListId | null {
  const list = params.get('list')
  if (isMailListId(list)) return list

  const tab = params.get('tab')
  if (tab === 'unread' || tab === 'starred') return tab

  // Archived was its own boolean param before it was a list.
  if (params.get('archived') === '1') return 'archived'

  const folder = params.get('folder')
  if (folder === 'Drafts') return 'draft'
  if (folder === 'Sent') return 'send'
  if (folder === 'Junk') return 'junk'
  if (folder === 'NP') return 'np'
  if (folder === 'Inbox') return 'inbox'
  return null
}
