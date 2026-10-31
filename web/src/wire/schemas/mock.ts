/**
 * Mock mail: disposable test mailboxes.
 *
 * Backend: crates/webapi/src/handlers/mock/rest.rs — every `/api/mock/*` handler
 *   - fn list_namespaces   -> json!({ "namespaces": Vec<NamespaceView> })
 *   - fn create_namespace  -> 201 NamespaceView
 *   - fn new_address       -> 201 MintedAddress
 *   - fn list_messages     -> MessageList
 *   - fn read_message      -> FullMessage
 *   - fn delete_message(s) -> json!({ "deleted": usize })
 *   - fn send              -> 201 SendResult (handlers/mock/send.rs)
 *   - fn rotate_smtp_token -> 201 SmtpCredentials
 *   - fn set_webhook       -> MockWebhook (handlers/mock/service.rs)
 *   - fn overview          -> AdminOverview (handlers/mock/admin.rs, GET /api/admin/mock)
 *   - fn save_settings     -> Settings (PUT /api/admin/mock/settings)
 *
 * Shapes read off the structs, none renamed:
 *   - `NamespaceView` (handlers/mock/service.rs) flattens `Namespace`
 *     (core-sidestate families/mock/namespace.rs) and adds `address`
 *   - `MessageMeta` (core-sidestate families/mock/messages.rs) flattens
 *     `Summary` (crates/mock/src/parsed.rs); `Direction` is kebab-case
 *   - `FullMessage` flattens `MessageMeta` and adds `text`, `html`, `dropped`
 *
 * The test fixtures are responses captured from production on 2.95.0, with
 * `uid` and `webhook_url` added by hand: those two fields arrived after it.
 * Times are milliseconds since the epoch.
 */

import { z } from '@/lib/zod'

/** Mirrors `NamespaceView`. */
export const mockNamespaceSchema = z.object({
  address: z.string(),
  bytes: z.number(),
  created_at: z.number(),
  dropped: z.number(),
  messages: z.number(),
  name: z.string(),
  owner: z.string(),
  smtp_enabled: z.boolean(),
  webhook_url: z.string().nullable(),
})

export type MockNamespace = z.infer<typeof mockNamespaceSchema>

export const mockNamespaceListSchema = z.object({
  namespaces: z.array(mockNamespaceSchema),
})

/** Mirrors `MintedAddress`. */
export const mockMintedAddressSchema = z.object({
  address: z.string(),
  minted_at: z.number(),
  namespace: z.string(),
  tag: z.string(),
})

export type MockMintedAddress = z.infer<typeof mockMintedAddressSchema>

/** Mirrors `Direction`, `#[serde(rename_all = "kebab-case")]`. */
export const mockDirectionSchema = z.enum(['in', 'captured-out'])

export type MockDirection = z.infer<typeof mockDirectionSchema>

export const mockLinkSchema = z.object({ text: z.string(), url: z.string() })

export type MockLink = z.infer<typeof mockLinkSchema>

export const mockAttachmentSchema = z.object({
  content_type: z.string(),
  filename: z.string(),
  size: z.number(),
})

export type MockAttachment = z.infer<typeof mockAttachmentSchema>

/** Mirrors `MessageMeta` with `Summary` flattened in. */
export const mockMessageMetaSchema = z.object({
  attachments: z.array(mockAttachmentSchema),
  cc: z.array(z.string()),
  codes: z.array(z.string()),
  direction: mockDirectionSchema,
  from: z.string(),
  id: z.string(),
  links: z.array(mockLinkSchema),
  message_id: z.string().nullable(),
  namespace: z.string(),
  rcpt: z.string(),
  received_at: z.number(),
  size: z.number(),
  subject: z.string(),
  tag: z.string(),
  to: z.array(z.string()),
  uid: z.number(),
})

export type MockMessageMeta = z.infer<typeof mockMessageMetaSchema>

/** Mirrors `MessageList`. */
export const mockMessageListSchema = z.object({
  dropped: z.number(),
  messages: z.array(mockMessageMetaSchema),
})

export type MockMessageList = z.infer<typeof mockMessageListSchema>

/** Mirrors `FullMessage`. */
export const mockFullMessageSchema = mockMessageMetaSchema.extend({
  dropped: z.number(),
  html: z.string().nullable(),
  text: z.string(),
})

export type MockFullMessage = z.infer<typeof mockFullMessageSchema>

/** `{"deleted": n}` from both delete routes. */
export const mockDeletedSchema = z.object({ deleted: z.number() })

/** Mirrors `SendResult` / `NotDelivered`. */
export const mockSendResultSchema = z.object({
  delivered: z.array(z.string()),
  id: z.string(),
  not_delivered: z.array(z.object({ address: z.string(), reason: z.string() })),
})

export type MockSendResult = z.infer<typeof mockSendResultSchema>

/** Mirrors `SmtpCredentials`. `host` is empty when MAILRS_HOSTNAME is unset. */
export const mockSmtpCredentialsSchema = z.object({
  host: z.string(),
  password: z.string(),
  username: z.string(),
})

export type MockSmtpCredentials = z.infer<typeof mockSmtpCredentialsSchema>

/** Mirrors `MockWebhook`. The secret signs every delivery, as for other webhooks. */
export const mockWebhookSchema = z.object({
  signing_secret: z.string(),
  url: z.string(),
})

export type MockWebhook = z.infer<typeof mockWebhookSchema>

/** Mirrors `Settings` (core-sidestate families/mock/settings.rs). */
export const mockSettingsSchema = z.object({
  create: z.enum(['all-users', 'admins-only']),
  enabled: z.boolean(),
  max_messages: z.number(),
  max_namespaces_per_user: z.number(),
  max_total_mb: z.number(),
  retention_days: z.number(),
})

export type MockSettings = z.infer<typeof mockSettingsSchema>

/** Mirrors `AdminOverview` (handlers/mock/admin.rs). `domain` is empty when none is configured. */
export const mockAdminOverviewSchema = z.object({
  domain: z.string(),
  settings: mockSettingsSchema,
  usage: z.object({
    bytes: z.number(),
    dropped: z.number(),
    namespaces: z.number(),
    owners: z.number(),
    received: z.number(),
    rejected_unknown_ns: z.number(),
  }),
})

export type MockAdminOverview = z.infer<typeof mockAdminOverviewSchema>
