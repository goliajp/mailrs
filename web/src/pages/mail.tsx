import { useAtomValue, useSetAtom } from 'jotai'
import { useCallback, useEffect } from 'react'

import { ComposeForm } from '@/components/compose-form'
import { MessageList } from '@/components/message-list'
import { MessageView } from '@/components/message-view'
import { Sidebar } from '@/components/sidebar'
import { fetchJson } from '@/lib/api'
import type { FolderInfo, MessageDetail, MessageSummary } from '@/lib/types'
import {
  composingAtom,
  foldersAtom,
  messagesAtom,
  selectedFolderAtom,
  selectedMessageDetailAtom,
  selectedMessageUidAtom,
} from '@/store/mail'

export function Mail() {
  const composing = useAtomValue(composingAtom)
  const setFolders = useSetAtom(foldersAtom)
  const setMessages = useSetAtom(messagesAtom)
  const selectedFolder = useAtomValue(selectedFolderAtom)
  const selectedUid = useAtomValue(selectedMessageUidAtom)
  const setMessageDetail = useSetAtom(selectedMessageDetailAtom)

  const loadFolders = useCallback(async () => {
    try {
      const data = await fetchJson<FolderInfo[]>('/mail/folders')
      setFolders(data)
    } catch {
      // keep current state
    }
  }, [setFolders])

  useEffect(() => {
    loadFolders()
  }, [loadFolders])

  useEffect(() => {
    if (!selectedFolder) return
    const load = async () => {
      try {
        const data = await fetchJson<MessageSummary[]>(
          `/mail/folders/${encodeURIComponent(selectedFolder)}/messages`
        )
        setMessages(data)
      } catch {
        setMessages([])
      }
    }
    load()
  }, [selectedFolder, setMessages])

  useEffect(() => {
    if (selectedUid === null) {
      setMessageDetail(null)
      return
    }
    const load = async () => {
      try {
        const data = await fetchJson<MessageDetail | null>(
          `/mail/messages/${selectedUid}`
        )
        setMessageDetail(data)
      } catch {
        setMessageDetail(null)
      }
    }
    load()
  }, [selectedUid, setMessageDetail])

  return (
    <div className="flex h-screen bg-white text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
      <Sidebar />
      <div className="flex w-80 shrink-0 flex-col border-r border-zinc-200 dark:border-zinc-800">
        <div className="border-b border-zinc-200 p-4 dark:border-zinc-800">
          <input
            type="text"
            placeholder="Search mail..."
            className="w-full rounded-md border border-zinc-200 bg-zinc-50 px-3 py-1.5 text-sm text-zinc-900 outline-none placeholder:text-zinc-400 focus:border-zinc-400 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:border-zinc-500"
          />
        </div>
        <MessageList />
      </div>
      {composing ? <ComposeForm /> : <MessageView />}
    </div>
  )
}
