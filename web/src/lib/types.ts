export type AccountInfo = {
  active: boolean
  address: string
  created_at: number
  display_name: string
  domain: string
  quota_bytes: number
  recovery_email: string
}

export type ActionInfoItem = {
  name: string
  type: string
  url?: string
}

export type AliasInfo = {
  active: boolean
  alias_type: string
  created_at: number
  domain: string
  id: number
  source_address: string
  target_address: string
}

export type AmountMention = {
  context: string
  currency?: string
  text: string
  value?: number
}

export type AttachmentInfo = {
  content_type: string
  filename: string
  size: number
}

export type CategoryCount = {
  category: string
  count: number
}

export type CheckResult = {
  details: string[]
  message: string
  name: string
  status: CheckStatus
}

// domain health check types
export type CheckStatus = 'fail' | 'pass' | 'skip' | 'warn'

export type ConnectionInfo = {
  addr: string
  authenticated?: string
  id: number
  lines: ConversationLine[]
  state: string
  tls: boolean
}

export type ConversationLine = {
  direction: 'client' | 'server'
  text: string
  timestamp: number
}

// conversation API types
export type ConversationSummary = {
  archived: boolean
  category: string
  flagged: boolean
  importance_level: string
  importance_score: number
  last_date: number
  last_sender: string
  message_count: number
  participants: string[]
  pinned: boolean
  requires_action: boolean
  snippet: string
  subject: string
  thread_id: string
  unread_count: number
}

export type DateMention = {
  context: string
  iso_date?: string
  text: string
}

export type DomainCheckReport = {
  checked_at: number
  checks: CheckResult[]
  domain: string
}

// admin API types
export type DomainInfo = {
  created_at: number
  name: string
}

export type EventInfo = {
  end_date?: string
  location?: string
  name: string
  start_date?: string
  url?: string
}

export type NewMessageEvent = {
  sender: string
  snippet: string
  subject: string
  thread_id: string
  type: 'NewMessage'
  user: string
}

export type Order = {
  currency?: string
  items: OrderItem[]
  merchant?: string
  order_date?: string
  order_number?: string
  status?: string
  total?: string
}

export type OrderItem = {
  name: string
  price?: string
  quantity?: number
}

export type PersonMention = {
  email?: string
  name: string
  role?: string
}

export type QueueEntry = {
  attempts: number
  created_at: number
  domain: string
  id: number
  last_error: null | string
  recipient: string
  sender: string
  status: string
  updated_at: number
}

export type QueueStats = {
  bounced: number
  delivered: number
  failed: number
  inflight: number
  pending: number
}

export type QuotaInfo = {
  address: string
  quota_bytes: number
}

export type ReactionSummary = {
  count: number
  emoji: string
  me: boolean
}

export type Reservation = {
  arrival_airport?: string
  departure_airport?: string
  end_date?: string
  flight_number?: string
  location?: string
  name?: string
  provider?: string
  reservation_id?: string
  start_date?: string
  status?: string
  type: string
}

export type ServerStatus = {
  active_connections: number
  queue?: QueueStats
  total_connections: number
  total_messages: number
  uptime_secs: number
}

export type SmtpEvent =
  | { addr: string; id: number; tls: boolean; type: 'ConnectionOpened' }
  | {
      command: string
      id: number
      state_before: string
      type: 'CommandReceived'
    }
  | { from: string; id: number; to: string[]; type: 'MessageQueued' }
  | {
      from: string
      id: number
      size: number
      to: string[]
      type: 'MessageDelivered'
    }
  | { id: number; reason: string; type: 'SpamRejected' }
  | { id: number; response: string; state_after: string; type: 'ResponseSent' }
  | { id: number; type: 'Authenticated'; username: string }
  | { id: number; type: 'ConnectionClosed' }
  | { id: number; type: 'TlsUpgraded' }

export type StructuredData = {
  actions?: ActionInfoItem[]
  events?: EventInfo[]
  orders?: Order[]
  reservations?: Reservation[]
}

export type ThreadMessage = {
  action_deadline: null | string
  action_items: string[]
  ai_analyzed: boolean
  amounts: AmountMention[]
  attachments: AttachmentInfo[]
  bimi_logo_url?: null | string
  category: string
  clean_text: null | string
  dates: DateMention[]
  flags: number
  has_tracking_pixel: boolean
  html_body: null | string
  id: number
  importance_level: string
  importance_score: number
  internal_date: number
  /// MRS-18: server-authoritative signal for invite-card mounting; populated
  /// from messages.invite_method via the conversations API. NULL when not
  /// an iTIP invite. Frontend uses this instead of inspecting attachments.
  invite_method?: null | string
  is_bulk_sender: boolean
  message_id: string
  new_content: null | string
  people: PersonMention[]
  recipients: string
  requires_action: boolean
  risk_reason: string
  risk_score: number
  sender: string
  sender_intent: string
  structured_data?: null | StructuredData
  subject: string
  summary: string
  text_body: null | string
  uid: number
}

// flag constants
export const FLAG_SEEN = 1
export const FLAG_ANSWERED = 2
export const FLAG_FLAGGED = 4
export const FLAG_DELETED = 8
export const FLAG_DRAFT = 16
