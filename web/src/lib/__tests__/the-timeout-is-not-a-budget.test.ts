import { describe, expect, it } from 'vitest'

/**
 * The suite's timeout is not a performance budget.
 *
 * Sixteen test files call `await import(…)` inside the test body, so
 * the first-time module transform is inside the timed region. On the
 * default five seconds, a laptop also running a release build failed
 * five of them, then two, then none — always "Test timed out in
 * 5000ms", never an assertion. A red that moves with the load is a red
 * people learn to skim past, and this repository has the note to prove
 * it (`.claude/rules` on load-dependent flakes).
 *
 * So the timeout is generous on purpose. This test says so out loud,
 * because a future reader finding `testTimeout: 20_000` would
 * reasonably wonder whether something here is slow, and the answer is
 * that the transform is, and that it is not what any of these tests
 * are about.
 *
 * Speed has a place to be asserted, and it is not here: the perf gates
 * live under each crate's `tests/perf_gate.rs`, run against a release
 * build, and carry real numbers.
 */
describe('the vitest timeout', () => {
  it('is long enough that a module transform cannot fail a test', async () => {
    const config = await import('../../../vite.config')
    const test = (config.default as { test?: { testTimeout?: number } }).test
    expect(test?.testTimeout ?? 5_000).toBeGreaterThanOrEqual(15_000)
  })
})
