// wrap assembled block HTML in a proper HTML email document
export function wrapInEmailTemplate(bodyHtml: string): string {
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<style>
  body { margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; font-size: 14px; line-height: 1.6; color: #222; background: #fff; }
  a { color: #0066cc; }
  pre { white-space: pre-wrap; word-break: break-word; }
  img { max-width: 100%; height: auto; }
  @media (prefers-color-scheme: dark) {
    body { background: #1a1a1a; color: #e0e0e0; }
    a { color: #5b9cf5; }
    blockquote { border-left-color: #555 !important; }
    hr { border-top-color: #444 !important; }
  }
</style>
</head>
<body>
<div style="max-width:600px;margin:0 auto;padding:16px">
${bodyHtml}
</div>
</body>
</html>`
}
