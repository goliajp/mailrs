# mailrs Roadmap (post-v3)

> v1 (server structure + stones extraction) ✅
> v2 (server polish) ✅
> v3 (stones × 6-dim audit + v2 cold backlog) ✅ closed at v1.7.30
>
> Now: two L1 tracks, run in parallel — they touch disjoint surfaces.

## L1-A — Stones perf 极致榨干 (v4)

**一句话**: 41 个 `mailrs-*` stones，每一个都做完 perf 深挖：profile
热点、对照 best-in-class 竞品（rust/go/c/c++/zig）、把 reproducible
bench 写到 PERFORMANCE.md、把可拿到的 perf 空间全部吃掉。

**结束态**: 任何一个 stone 拿出来都没有"明显还能优化"的点 — 文档里标
`best-in-class (vs X)` 或 `first-in-Rust + no competitor in language`。

**工作方式**: 不是 41 个 stone 各做一遍 6-dim 表格（v3 已经做了），是按
**hot path** 排优先级：高频路径 / 大份额 CPU 的 stone 先深挖。

| 优先级 | Stones | Rationale |
|---|---|---|
| **P0** (per-message 热路径) | smtp-proto, imap-proto, rfc5322, mime, smtp-codec, imap-codec, imap-format | 每条邮件经过 N 次 |
| **P1** (auth + storage 热路径) | dkim, spf, arc, dmarc, rate-limit, maildir, mailbox, inbound, smtp-client | 每条邮件经过 1 次但 CPU-heavy |
| **P2** (outbound + scrubbing) | outbound-queue, shield, clean, postmaster, intelligence, attachment-extract, sieve | 异步 / batch |
| **P3** (cold / 配置类) | acme, tls-reload, dnsbl, clamav, jmap, dav, ical, mta-sts, tls-rpt, srs, rfc2047, rfc2231, arf, backoff, dns, webhook-signature, auth-guard, delivery-executor, clean | 启动期 / 配置 / 不在 message 热路径 |

## L1-B — CI/CD: GitHub Actions docker-compose 部署 (v5)

**一句话**: 把 `./scripts/release.sh` 的"本地 cargo zigbuild + ssh upload
+ remote docker restart"链路，迁移到"push tag → GitHub Actions cargo
test + docker build + ghcr push → remote pulls + docker-compose up"。

**结束态**: `git push origin v1.x.y` 即触发 CI 跑 test + build + push
image；远端只需要拉镜像 + `docker-compose up -d`。本地 release.sh 保留
作 fallback 直到 CI 链路稳定（连续 ≥ 3 个 release 不出意外）后弃用。

**Devops 边界（不可破坏）**:
- prod `t02.golia.jp` 当前跑 v1.7.30，**不能中断**
- 现有 docker-compose 服务（postgres / valkey / mailrs）不能换 schema
- secrets 全部经 GitHub repo secrets，**不写明文进 workflow 文件**
- 切换期保留 `scripts/release.sh` 作 fallback，直到 CI 链路稳跑过 ≥3 release

## v4 / v5 顺序

**并行**。两者触碰文件集不相交：
- v5: `.github/workflows/`, `docker-compose.prod.yml`, `Dockerfile` micro-tweaks
- v4: 每个 stone 的 `src/*.rs` + `benches/*.rs` + `BUDGETS.md`

v5 先做（小、独立、给 v4 加速 release 节奏）—— v5 没完成不阻塞 v4 起手，
但 v5 落地后 v4 的每个 stone polish 都能享受 CI 跑全工作区 test
（本地 cargo test --workspace 现在已经要 12+ 分钟）。

## v4 / v5 各自的 L2 边界（定下不动）

### v5 边界

1. `Dockerfile` 优化 multi-stage 减体积 + 加 ghcr-friendly LABEL
2. `.github/workflows/release.yml` — push `v*` tag 触发
3. `.github/workflows/test.yml` — push to develop / main 触发 cargo test + bun test
4. `docker-compose.prod.yml` — 远端用，`image: ghcr.io/goliajp/mailrs:<tag>`，不再 build from path
5. `scripts/release.sh --ci` mode — 跑本地 test → push tag，让 CI 接管
6. GitHub repo secrets 文档化（`CI-SETUP.md`）

### v4 边界

1. `scripts/perf-profile.sh` — 通用 samply / cargo-flamegraph harness
2. P0 7 stones × profile + optimize + bench refresh + PERFORMANCE.md
3. P1 9 stones × 同上
4. P2 7 stones × 同上
5. P3 18 stones — 单批 spot-check（不在热路径，不需深挖）+ PERFORMANCE.md 标 first-in-Rust
6. PERFORMANCE.md 终审 + ROADMAP.md "v4 closed" 段

## L4 Triggers

| From → To | Trigger |
|---|---|
| v5.0 → v5.1 | `Dockerfile` 优化完 + ghcr push 通 — ✅ `test.yml` 首次 green at commit `564f1fe` (run 26395568983, 8m24s, 2026-05-25) |
| v5.1 → v5.2 | `release.yml` workflow 跑通一个 dry-run tag |
| v5.2 → v5.3 | 1 个 real release 经 CI 跑完 + prod 健康 |
| v5.3 → v5 closed | 连续 3 个 release 经 CI 无干预 |
| v4.0 → v4.1 | `perf-profile.sh` + 1 个 P0 stone 走通流程 |
| v4.1 → v4.2 | P0 7 stones 全过 |
| v4.2 → v4.3 | P1 9 stones 全过 |
| v4.3 → v4.4 | P2 7 stones 全过 |
| v4.4 → v4 closed | P3 spot-check + PERFORMANCE.md 终审 |

## 我的执行准则（继承 v3）

- 不在 hot 中重新规划；step 失败 → 停 + 回报
- 每完成一个 stone / 每完成一个 workflow patch 立即 commit；不悄悄扩 scope
- 竞品 perf 数字必须有 reproducible 命令；不存在的不写"first-in-Rust"
- CI 改动绝不在 v3.x prod hotfix 路径里做；prod 出问题永远用本地 release.sh fallback

## v6 — Polish pass (2026-05-26 → 2026-05-27) — closed

7 checkpoint linear sweep; "no new features, close existing
commitments". See `.claude/rfcs/20260526-polish-pass-v6.md` for the
full plan; the deliverable docs live at:

- ckpt 0 — `health-check-2026-05-26.md` (no P0 blocker)
- ckpt 1 — 10 god-file splits committed (`6ca1eaa` → `08c1b93`)
- ckpt 2 — `mail-auth` + `mail-parser` removed from server +
  outbound-queue; mailrs-spf/dkim/arc/dmarc primary path
  (`2841088` → `97ca1f8` + `e269dcd` h= fix)
- ckpt 3 — P2 stones measured numbers in `PERFORMANCE.md`; P3
  spot-check labels confirmed; PEM-cache fix on dkim_sign
  (`172dde2` + `7b77d12`)
- ckpt 4 — `REFACTOR-V2-v6-ckpt4-security.md` (OWASP 10/10 ✅);
  3 new metric names (`mailrs_smtp_connections_total`,
  `mailrs_outbound_queue_depth`, `mailrs_outbound_delivery_seconds`)
- ckpt 5 — `REFACTOR-V2-v6-ckpt5-coverage.md` (workspace 82.42 %
  lib coverage); proptest added to rfc5322 + mime to bring all 4
  parser crates to coverage
- ckpt 6 — `release.yml` fixed (multi-arch QEMU build that timed
  out at 1h replaced by single-arch linux/amd64; ~11 min instead
  of 1h+); v1.7.32 release.yml all 3 jobs green
- ckpt 7 — every published stone has a `Performance` section in
  its README pointing to `BUDGETS.md` + workspace
  `PERFORMANCE.md`; ARCHITECTURE.md stone count (41) already
  correct; this section closes the loop

**Carry-overs to v7** (not blockers, just follow-ups):

- mailrs-dkim crypto backend swap (`rsa` 0.9 → `aws-lc-rs`) for
  ~3× RSA-2048 sign speed-up on outbound dkim path
- Per-stage inbound pipeline timing histograms (`mailrs-inbound`
  Stage trait needs a timing facade first)
- TLS-RPT / MTA-STS / ARC Grafana dashboard JSON (waits on
  `devops.golia.jp` Grafana → mailrs `/metrics` plumbing)
- Coverage gaps on live-DNS / live-SMTP / PG-backed modules
  (postmaster check submodules, outbound-queue worker, smtp-client
  connection) — needs a mock-DNS + docker-pg fixture layer
- ckpt 6 trigger says "3 successful release.yml runs in a row";
  v1.7.32 is #1 of 3 — the remaining 2 will accumulate as future
  releases ship through the now-working CI
