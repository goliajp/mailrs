// Writes web/src/lib/markdown-samples.json: markdown → the HTML the web send
// path puts in `html_body`, plus the email wrapper. The phone clients read
// copies of it (ios/MailrsTests, android/app/src/test/resources) and must
// produce the same bytes; scripts/check-markdown-samples.sh keeps the copies
// identical. Run after changing the renderer: `bun scripts/markdown-samples.ts`.
import { writeFileSync } from 'node:fs'

import { wrapInEmailTemplate } from '../src/components/composer/renderers/email-template'
import { renderMarkdownForSend } from '../src/lib/render-markdown'

const cases: Array<[string, string]> = [
  ['plain', 'Hello there'],
  ['bold', 'Hello **big**'],
  ['bold underscores', '__under__ and **star**'],
  ['italic', 'say _hi_ and *there*'],
  ['nested', '**bold _both_**'],
  ['italic around bold', '*a **b** c*'],
  ['intraword star', 'a*b*c'],
  ['intraword underscore', 'snake_case_word and **x**'],
  ['underscore run', '_a_b_'],
  ['unclosed', '**unclosed'],
  ['backslash', 'a\\*b'],
  ['code span', '`a<b` and `x`'],
  ['link', 'see [docs](https://example.com)'],
  ['link with formatting', '[**bold** link](https://e.example)'],
  ['link query', '[t](https://e.example/a?b=1&c="2")'],
  ['mailto link', '[m](mailto:a@b.jp)'],
  ['toolbar link placeholder', 'see [docs](https://)'],
  ['autolink', 'visit https://example.com/x?y=1 now'],
  ['autolink ampersand', 'https://a.example/b?c=1&d=2'],
  ['autolink trailing dot', 'go to https://example.com/x.'],
  ['autolink parens', 'link (https://example.com/a) end'],
  ['autolink underscores', 'https://example.com/a_b_c'],
  ['www', 'see www.example.com.'],
  ['email', 'mail me at a.b@golia.jp please'],
  ['escaping', 'a < b & c > d "q" \'s\''],
  ['entity kept', '&amp; already'],
  ['line break', 'line one\nline two'],
  ['paragraphs', 'para one\n\npara two'],
  ['blank lines kept', 'a\n\n\nb'],
  ['heading', '# H1\ntext'],
  ['heading six', '###### H6'],
  ['not a heading', '####### seven'],
  ['quote', '> a\n> b'],
  ['quote no space', '>no space'],
  ['reply quote', 'Thanks!\n\nOn Mon, A wrote:\n> q1\n> q2'],
  ['bullets', '- a\n- b'],
  ['star bullets', '* star item\n* two'],
  ['numbered', '1. a\n2. b'],
  ['numbered from three', '3. three\n4. four'],
  ['list after paragraph', 'text\n- a'],
  ['numbered after paragraph', 'para\n1. one'],
  ['two is not a list after paragraph', 'para\n2. two'],
  ['tab', 'tab\tsep'],
  ['japanese', '**件名**です。_よろしく_'],
  ['injection script', '<script>alert(1)</script>'],
  ['injection tag', 'a <b onclick="x()">b</b> c'],
  ['injection javascript link', '[x](javascript:alert(1))'],
  ['injection data link', '[x](data:text/html,<b>x</b>)'],
  ['injection relative link', '[x](/admin)'],
  ['empty', ''],
]

const samples = cases.map(([name, markdown]) => ({ html: renderMarkdownForSend(markdown), markdown, name }))
const wrap = { html: wrapInEmailTemplate('<p>x</p>\n'), inner: '<p>x</p>\n' }
writeFileSync(
  new URL('../src/lib/markdown-samples.json', import.meta.url),
  JSON.stringify({ samples, wrap }, null, 2) + '\n'
)
console.log(`wrote ${samples.length} samples`)
