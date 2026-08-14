import { describe, expect, it } from 'vitest'

import { htmlBodyPaintsNothing } from '@/lib/email-split'

/**
 * The mail that produced this: subject "The small onboarding step that
 * creates outsized friction", from connect@customdomain.ai, 2026-08-14.
 * An Odoo mass-mailing sent through SES whose body never made it into
 * either MIME part — 2,785 bytes of HTML of which 2.4 kB is a `<style>`
 * block in the head, and a `<body>` holding a hidden preheader and a
 * tracking gif. It arrived intact, was parsed correctly, and rendered as
 * a white box, because a non-empty `html_body` is what the reader pane
 * tests before choosing the HTML branch.
 */
const BLANK_ODOO_BODY = `
        <!DOCTYPE html>
        <html>
            <head>
                <meta http-equiv="Content-Type" content="text/html; charset=UTF-8"/>
                <style>
.o_layout { font-family: Arial; }
@media screen and (max-width: 768px) { td { text-align: center !important; } }
</style>
            </head>
            <body>
                <div style="display:none; font-size:1px; height:0px; width:0px; opacity:0">
                    A small setup step can quietly become a major source of lost activation.
                </div>

<img src="https://customdomain.ai/mail/track/16483/8e354a234a4d/blank.gif"/>
</body>
        </html>`

describe('htmlBodyPaintsNothing', () => {
  it('calls the empty Odoo mailing empty', () => {
    expect(htmlBodyPaintsNothing(BLANK_ODOO_BODY)).toBe(true)
  })

  it('does not count a stylesheet as something to read', () => {
    expect(htmlBodyPaintsNothing('<style>p { color: red }</style><body></body>')).toBe(true)
  })

  it('does not count a hidden preheader as something to read', () => {
    expect(htmlBodyPaintsNothing('<div style="display:none">preheader</div>')).toBe(true)
    expect(htmlBodyPaintsNothing('<div style="visibility: hidden">preheader</div>')).toBe(true)
  })

  it('counts ordinary text', () => {
    expect(htmlBodyPaintsNothing('<p>hello</p>')).toBe(false)
  })

  it('counts text that sits beside a hidden preheader', () => {
    expect(htmlBodyPaintsNothing('<div style="display:none">pre</div><p>the actual mail</p>')).toBe(
      false
    )
  })

  /**
   * A body of one image is not empty — that is what an image-only
   * newsletter is. What separates it from a beacon is that it says how
   * big it is, or what it shows.
   */
  it('counts an image that declares a size', () => {
    expect(htmlBodyPaintsNothing('<img src="https://e.example/a.png" width="600">')).toBe(false)
    expect(htmlBodyPaintsNothing('<img src="https://e.example/a.png" style="width:600px">')).toBe(
      false
    )
    expect(htmlBodyPaintsNothing('<img src="https://e.example/a.png" width="100%">')).toBe(false)
  })

  it('counts an image that carries alt text', () => {
    expect(htmlBodyPaintsNothing('<img src="https://e.example/a.png" alt="Our new plan">')).toBe(
      false
    )
  })

  /**
   * An embedded part rather than a fetch. A mail client writing an inline
   * image often declares nothing about it, and it is still the message.
   */
  it('counts an inline image whatever it declares', () => {
    expect(htmlBodyPaintsNothing('<img src="cid:logo">')).toBe(false)
    expect(htmlBodyPaintsNothing('<img src="data:image/png;base64,iVBOR">')).toBe(false)
  })

  it('does not count a declared 1x1 beacon', () => {
    expect(
      htmlBodyPaintsNothing('<img src="https://e.example/open.gif" width="1" height="1">')
    ).toBe(true)
  })

  it('counts a table, which is how newsletters lay themselves out', () => {
    expect(htmlBodyPaintsNothing('<table><tr><td>hi</td></tr></table>')).toBe(false)
  })

  it('treats an empty document as empty rather than throwing', () => {
    expect(htmlBodyPaintsNothing('')).toBe(true)
    expect(htmlBodyPaintsNothing('   \n  ')).toBe(true)
  })
})
