/**
 * Mock mail: disposable test mailboxes under `/api/mock/*`.
 *
 * Backend: crates/webapi/src/handlers/mock/rest.rs
 */

import type {
  MockFullMessage,
  MockMessageList,
  MockMintedAddress,
  MockNamespace,
  MockSendResult,
  MockSmtpCredentials,
} from '../schemas/mock'

import { getToken } from '@/store/auth'

import { wireFetch } from '../client'
import {
  mockDeletedSchema,
  mockFullMessageSchema,
  mockMessageListSchema,
  mockMintedAddressSchema,
  mockNamespaceListSchema,
  mockNamespaceSchema,
  mockSendResultSchema,
  mockSmtpCredentialsSchema,
} from '../schemas/mock'
import { emptyResponseSchema } from '../schemas/mutations'

export type MockSendInput = {
  fromTag?: string
  html?: string
  subject: string
  text: string
  to: string[]
}

/** Raw `.eml` link. A plain `<a href>` cannot send a header, so the token rides in the query. */
export function mockRawUrl(ns: string, id: string): string {
  const token = encodeURIComponent(getToken() ?? '')
  return `/api${messagePath(ns, id)}/raw?token=${token}`
}

/** `POST /api/mock/namespaces` → 201 `NamespaceView`. */
export function wireCreateMockNamespace(name: string): Promise<MockNamespace> {
  return wireFetch(mockNamespaceSchema, {
    body: { name },
    method: 'POST',
    path: '/mock/namespaces',
  })
}

/** `DELETE /api/mock/namespaces/{ns}/messages/{id}` → `{deleted}`. */
export async function wireDeleteMockMessage(ns: string, id: string): Promise<number> {
  const out = await wireFetch(mockDeletedSchema, { method: 'DELETE', path: messagePath(ns, id) })
  return out.deleted
}

/** `DELETE /api/mock/namespaces/{ns}` → 204. */
export async function wireDeleteMockNamespace(ns: string): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    method: 'DELETE',
    path: nsPath(ns),
  })
}

/** `GET /api/mock/namespaces/{ns}/messages` → newest first, with the namespace's dropped count. */
export function wireListMockMessages(ns: string, signal?: AbortSignal): Promise<MockMessageList> {
  return wireFetch(mockMessageListSchema, {
    path: `/mock/namespaces/${encodeURIComponent(ns)}/messages`,
    signal,
  })
}

/** `GET /api/mock/namespaces` → the caller's namespaces. 404 when the deployment has no mock domain. */
export async function wireListMockNamespaces(signal?: AbortSignal): Promise<MockNamespace[]> {
  const out = await wireFetch(mockNamespaceListSchema, { path: '/mock/namespaces', signal })
  return out.namespaces
}

/** `POST /api/mock/namespaces/{ns}/addresses` → 201 `MintedAddress`. No tag mints a random one. */
export function wireMintMockAddress(ns: string, tag?: string): Promise<MockMintedAddress> {
  return wireFetch(mockMintedAddressSchema, {
    body: { tag },
    method: 'POST',
    path: `/mock/namespaces/${encodeURIComponent(ns)}/addresses`,
  })
}

/** `POST /api/mock/namespaces/{ns}/send` → 201 `SendResult`. Nothing leaves the deployment. */
export function wireMockSend(ns: string, input: MockSendInput): Promise<MockSendResult> {
  return wireFetch(mockSendResultSchema, {
    body: {
      from_tag: input.fromTag,
      html: input.html,
      subject: input.subject,
      text: input.text,
      to: input.to,
    },
    method: 'POST',
    path: `/mock/namespaces/${encodeURIComponent(ns)}/send`,
  })
}

/** `GET /api/mock/namespaces/{ns}/messages/{id}` → `FullMessage`. */
export function wireReadMockMessage(
  ns: string,
  id: string,
  signal?: AbortSignal
): Promise<MockFullMessage> {
  return wireFetch(mockFullMessageSchema, { path: messagePath(ns, id), signal })
}

/** `POST /api/mock/namespaces/{ns}/smtp-token` → 201 credentials; replaces any earlier password. */
export function wireRotateMockSmtpToken(ns: string): Promise<MockSmtpCredentials> {
  return wireFetch(mockSmtpCredentialsSchema, {
    method: 'POST',
    path: `/mock/namespaces/${encodeURIComponent(ns)}/smtp-token`,
  })
}

function messagePath(ns: string, id: string): string {
  return `/mock/namespaces/${encodeURIComponent(ns)}/messages/${encodeURIComponent(id)}`
}

function nsPath(ns: string): string {
  return `/mock/namespaces/${encodeURIComponent(ns)}`
}
