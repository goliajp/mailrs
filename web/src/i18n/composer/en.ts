/** Composer entries (all three shells), spread into `en`; kept apart to keep each file small. */
export const composerEn = {
  'composer.editFields': 'Edit recipients and subject',
  'composer.format.bold': 'Bold (Ctrl+B)',
  'composer.format.code': 'Code',
  'composer.format.italic': 'Italic (Ctrl+I)',
  'composer.format.link': 'Link (Ctrl+K)',
  'composer.format.list': 'Bulleted list',
  'composer.format.numbered': 'Numbered list',
  'composer.format.quote': 'Quote',
  'composer.format.toolbar': 'Formatting',
  'composer.from': 'From',
  'composer.hideFields': 'Hide recipients and subject',
} as const

export type ComposerKey = keyof typeof composerEn
