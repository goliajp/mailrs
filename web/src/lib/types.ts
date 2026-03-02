export type SmtpEvent =
  | { type: 'ConnectionOpened'; id: number; addr: string; tls: boolean }
  | {
      type: 'CommandReceived'
      id: number
      command: string
      state_before: string
    }
  | { type: 'ResponseSent'; id: number; response: string; state_after: string }
  | { type: 'TlsUpgraded'; id: number }
  | { type: 'Authenticated'; id: number; username: string }
  | {
      type: 'MessageDelivered'
      id: number
      from: string
      to: string[]
      size: number
    }
  | { type: 'SpamRejected'; id: number; reason: string }
  | { type: 'MessageQueued'; id: number; from: string; to: string[] }
  | { type: 'ConnectionClosed'; id: number }

export type ServerStatus = {
  uptime_secs: number
  active_connections: number
  total_connections: number
  total_messages: number
  queue?: QueueStats
}

export type QueueStats = {
  pending: number
  inflight: number
  delivered: number
  failed: number
  bounced: number
}

export type QueueEntry = {
  id: number
  sender: string
  recipient: string
  domain: string
  status: string
  attempts: number
  last_error: string | null
  created_at: number
  updated_at: number
}

export type ConnectionInfo = {
  id: number
  addr: string
  tls: boolean
  authenticated?: string
  state: string
  lines: ConversationLine[]
}

export type ConversationLine = {
  direction: 'client' | 'server'
  text: string
  timestamp: number
}

// mail API types
export type FolderInfo = {
  name: string
  total: number
  unseen: number
  uidnext: number
}

export type MessageSummary = {
  uid: number
  sender: string
  recipients: string
  subject: string
  size: number
  flags: number
  internal_date: number
}

export type AttachmentInfo = {
  filename: string
  content_type: string
  size: number
}

export type MessageDetail = MessageSummary & {
  text_body: string | null
  html_body: string | null
  attachments: AttachmentInfo[]
}

// admin API types
export type DomainInfo = {
  name: string
  created_at: number
}

export type AccountInfo = {
  address: string
  domain: string
  display_name: string
  active: boolean
  created_at: number
}

// conversation API types
export type ConversationSummary = {
  thread_id: string
  subject: string
  participants: string[]
  message_count: number
  unread_count: number
  last_date: number
}

export type ThreadMessage = {
  id: number
  uid: number
  sender: string
  recipients: string
  subject: string
  flags: number
  internal_date: number
  message_id: string
  text_body: string | null
  html_body: string | null
  attachments: AttachmentInfo[]
}

export type NewMessageEvent = {
  type: 'NewMessage'
  user: string
  thread_id: string
  sender: string
  subject: string
  snippet: string
}

// flag constants
export const FLAG_SEEN = 1
export const FLAG_ANSWERED = 2
export const FLAG_FLAGGED = 4
export const FLAG_DELETED = 8
export const FLAG_DRAFT = 16
