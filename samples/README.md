# Email Samples

Test emails downloaded from production mailbox for verifying mailrs MIME handling and display.

`.eml` files are gitignored (contain real email content).

## How to populate

```bash
python3 scripts/fetch-samples.py
```

## Sample categories

| File | Type | Charset | Description |
|------|------|---------|-------------|
| multipart_alt_ja.eml | multipart/alternative | utf-8 | Japanese text+html |
| multipart_alt_zh.eml | multipart/alternative | utf-8 | Chinese text+html |
| multipart_alt_en.eml | multipart/alternative | utf-8 | English text+html |
| text_plain.eml | text/plain | utf-8 | Plain text only |
| text_html.eml | text/html | iso-2022-jp | HTML only, legacy JP encoding |
| multipart_mixed_attachment.eml | multipart/mixed | utf-8 | With attachment parts |
| multipart_signed.eml | multipart/signed | utf-8 | S/MIME signed (pkcs7) |
| iso2022jp.eml | text/html | iso-2022-jp | Legacy Japanese encoding |
| large_200k.eml | multipart/alternative | utf-8 | Large email (~260KB) |
| small_5k.eml | multipart/alternative | utf-8 | Small email (~5KB) |
| has_cc.eml | multipart/alternative | utf-8 | Multiple recipients (Cc) |
| has_reply.eml | multipart/alternative | utf-8 | Thread reply (In-Reply-To) |
| sent_*.eml | text/html | utf-8 | Outbound emails from sent folder |
