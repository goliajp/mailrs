import type { WireFraudVerdict } from '@/wire/schemas/fraud'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { chosenLangAtom } from '@/i18n'
import { fetchFraudVerdict } from '@/wire/endpoints/fraud'
import { wireFraudVerdictSchema } from '@/wire/schemas/fraud'

import { FraudVerdictPanel } from '../fraud-verdict-panel'

vi.mock('@/wire/endpoints/fraud', () => ({
  fetchFraudVerdict: vi.fn(),
  releaseQuarantined: vi.fn(),
}))

afterEach(cleanup)

const layer = (name: string, outcome: 'fail' | 'pass', reasons: object[], detail: string) => ({
  detail,
  name,
  outcome,
  reasons,
  score: 0,
})

/** A verdict as `mailrs_inbound::FraudVerdict` serialises it. */
const HELD = {
  layers: [
    layer(
      'transport',
      'pass',
      [
        { code: 'auth-check', params: { check: 'SPF', result: 'pass' } },
        { code: 'auth-check', params: { check: 'DKIM', result: 'pass' } },
        { code: 'auth-check', params: { check: 'DMARC', result: 'pass' } },
      ],
      'SPF pass, DKIM pass, DMARC pass (policy n/a (verified))'
    ),
    layer(
      'identity',
      'fail',
      [{ code: 'claims-our-name' }],
      'display name claims this organisation'
    ),
    layer('provenance', 'pass', [], 'nothing unusual in the headers'),
    layer('content', 'fail', [{ code: 'a-rule-added-later' }], 'the server says what it saw'),
  ],
  placement: [{ code: 'junk-score', params: { score: '9.5', threshold: '5.0' } }],
  quarantined: true,
  rules_version: '2026-08-29.1',
  score: 9.5,
}

function show(verdict: object, lang: 'en' | 'zh') {
  vi.mocked(fetchFraudVerdict).mockResolvedValue(
    wireFraudVerdictSchema.parse(verdict) as WireFraudVerdict
  )
  const store = createStore()
  store.set(chosenLangAtom, lang)
  render(
    <Provider store={store}>
      <QueryClientProvider client={new QueryClient()}>
        <FraudVerdictPanel messageId="m1@x" threadId="t1" />
      </QueryClientProvider>
    </Provider>
  )
}

describe('the verdict panel', () => {
  it('words every reason in the reader’s language', async () => {
    show(HELD, 'zh')
    expect(await screen.findByText('显示名自称是本组织。')).toBeInTheDocument()
    expect(screen.getByText('身份')).toBeInTheDocument()
    expect(screen.getByText('SPF：pass')).toBeInTheDocument()
    expect(screen.getByText('没有发现问题。')).toBeInTheDocument()
    expect(screen.getByText('各项检查合计 9.5 分，达到垃圾邮件阈值 5.0。')).toBeInTheDocument()
  })

  it('falls back to the server’s words for a code it has no sentence for', async () => {
    show(HELD, 'en')
    expect(await screen.findByText('the server says what it saw')).toBeInTheDocument()
  })

  it('reads a verdict stored before reasons existed', async () => {
    const old = { ...HELD, layers: HELD.layers.map((l) => ({ ...l, reasons: undefined })) }
    delete (old as { placement?: unknown }).placement
    show(old, 'en')
    expect(await screen.findByText('display name claims this organisation')).toBeInTheDocument()
  })

  it('says only why mail nobody suspected was sorted where it is', async () => {
    show(
      {
        layers: [],
        placement: [{ code: 'sorted-by-your-rule', params: { class: 'promotion' } }],
        quarantined: false,
        rules_version: '2026-08-29.1',
        score: 0,
      },
      'en'
    )
    expect(
      await screen.findByText('Your rule for this sender puts it in Promotions.')
    ).toBeInTheDocument()
    expect(screen.queryByText('Identity')).not.toBeInTheDocument()
  })
})
