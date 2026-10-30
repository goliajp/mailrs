import type { Extensions } from '@tiptap/react'

import Link from '@tiptap/extension-link'
import Placeholder from '@tiptap/extension-placeholder'
import StarterKit from '@tiptap/starter-kit'

import { wireUploadInlineImage } from '@/wire/endpoints/mail'

/// The signature editor's extensions: text, lists and links, nothing a
/// signature has no use for.
export function createMinimalExtensions(placeholder?: string): Extensions {
  return [
    StarterKit.configure({
      blockquote: false,
      codeBlock: false,
      heading: false,
      horizontalRule: false,
      link: false,
      underline: false,
    }),
    Link.configure({ autolink: true, openOnClick: false }),
    Placeholder.configure({
      placeholder: placeholder ?? '',
    }),
  ]
}

export async function uploadInlineImage(file: File): Promise<null | string> {
  try {
    const data = await wireUploadInlineImage(file)
    if (data.success && data.url) return data.url
  } catch {
    // fallback handled by caller
  }
  return null
}
