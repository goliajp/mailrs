# mailrs performance audit

Living workspace for production performance work. Not shipped, not user-facing — only used to record measurements and drive fixes.

## Layout

```
perfs/
├── README.md              ← you are here (mode of operation)
├── TREE.md                ← whole-system map: every page → assets/APIs → numbers → topic links
├── topics/                ← one file per investigation
│   ├── _template.md       ← copy this when opening a new topic
│   └── NN-slug.md         ← deep-dive per problem (status, hypotheses, decisions, verification)
├── scripts/               ← measurement tools, all reproducible
│   ├── timing.sh          ← curl-level timing for a single URL (DNS/TCP/TLS/TTFB/total)
│   └── page-perf.js       ← puppeteer SPA navigation timing across all pages
└── data/                  ← raw measurements, dated, immutable once written
    └── YYYY-MM-DD/        ← one folder per measurement run
```

## Mode of operation

1. **Establish the map.** `TREE.md` is the canonical view of where time goes. Every leaf is either ✓ healthy, · informational, or ⚠ links to a topic file. New surface area gets added to TREE before any deep-dive.
2. **Open a topic per anomaly.** Anything ⚠ in TREE.md gets its own `topics/NN-slug.md` — copy `_template.md`, give it a number (next free), set status `open`. The topic file owns hypotheses, the investigation log, the decision, and the post-fix verification numbers.
3. **Measure first, then theorize.** Numbers go into `data/<date>/` *before* we form opinions. Topic files cite data files by path. Hypotheses without supporting evidence are explicitly marked.
4. **Re-measure to close.** Closing a topic requires a `data/<date>/` re-run that shows the metric moved. Status ladder: `open → investigating → fix proposed → fixed (vX.Y.Z)`.
5. **Snapshots are append-only.** Never overwrite a previous `data/<date>/` directory; create a new dated one. Topic files may rewrite freely.

## Reproduce all numbers

```bash
# 1. login + token (once per session)
TOKEN=$(curl -s -X POST https://mail.golia.ai/api/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"address":"…","password":"…"}' | jq -r .token)
export TOKEN

# 2. one URL
./scripts/timing.sh "label" GET 'https://mail.golia.ai/api/...'

# 3. full SPA pass
bun scripts/page-perf.js

# 4. lighthouse on a public route
bunx --bun lighthouse https://mail.golia.ai/login \
  --output=json --output-path=data/$(date +%F)/login.lh.json \
  --chrome-flags="--headless=new" \
  --form-factor=desktop --throttling-method=provided --screenEmulation.disabled
```

## Conventions

- All numbers in TREE.md and topics are **medians of 3 runs** unless explicitly labeled "(single run)".
- Network conditions, account, and prod version are recorded at the top of TREE.md so old measurements stay interpretable.
- `topics/` numbering is monotonically increasing and never reused, even after a topic closes. The number is part of the topic's identity.
