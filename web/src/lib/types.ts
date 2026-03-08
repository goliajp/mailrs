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

export type AttachmentInfo = {
  filename: string
  content_type: string
  size: number
}

export type PersonMention = {
  name: string
  email?: string
  role?: string
}

export type DateMention = {
  text: string
  iso_date?: string
  context: string
}

export type AmountMention = {
  text: string
  value?: number
  currency?: string
  context: string
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

export type AliasInfo = {
  id: number
  source_address: string
  target_address: string
  domain: string
  alias_type: string
  active: boolean
  created_at: number
}

export type QuotaInfo = {
  address: string
  quota_bytes: number
}

// conversation API types
export type ConversationSummary = {
  thread_id: string
  subject: string
  participants: string[]
  message_count: number
  unread_count: number
  last_date: number
  category: string
  flagged: boolean
  snippet: string
  pinned: boolean
  archived: boolean
  importance_level: string
  importance_score: number
}

export type CategoryCount = {
  category: string
  count: number
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
  category: string
  risk_score: number
  risk_reason: string
  summary: string
  people: PersonMention[]
  dates: DateMention[]
  amounts: AmountMention[]
  action_items: string[]
  ai_analyzed: boolean
  clean_text: string | null
  new_content: string | null
  importance_level: string
  importance_score: number
  is_bulk_sender: boolean
  has_tracking_pixel: boolean
  requires_action: boolean
  sender_intent: string
  action_deadline: string | null
  structured_data?: StructuredData | null
}

export type StructuredData = {
  reservations?: Reservation[]
  orders?: Order[]
  events?: EventInfo[]
  actions?: ActionInfoItem[]
}

export type Reservation = {
  type: string
  name?: string
  reservation_id?: string
  status?: string
  start_date?: string
  end_date?: string
  location?: string
  provider?: string
  departure_airport?: string
  arrival_airport?: string
  flight_number?: string
}

export type Order = {
  order_number?: string
  merchant?: string
  order_date?: string
  status?: string
  items: OrderItem[]
  total?: string
  currency?: string
}

export type OrderItem = {
  name: string
  quantity?: number
  price?: string
}

export type EventInfo = {
  name: string
  start_date?: string
  end_date?: string
  location?: string
  url?: string
}

export type ActionInfoItem = {
  type: string
  name: string
  url?: string
}

export type NewMessageEvent = {
  type: 'NewMessage'
  user: string
  thread_id: string
  sender: string
  subject: string
  snippet: string
}

// domain health check types
export type CheckStatus = 'pass' | 'warn' | 'fail' | 'skip'

export type CheckResult = {
  name: string
  status: CheckStatus
  message: string
  details: string[]
}

export type DomainCheckReport = {
  domain: string
  checks: CheckResult[]
  checked_at: number
}

// flag constants
export const FLAG_SEEN = 1
export const FLAG_ANSWERED = 2
export const FLAG_FLAGGED = 4
export const FLAG_DELETED = 8
export const FLAG_DRAFT = 16
