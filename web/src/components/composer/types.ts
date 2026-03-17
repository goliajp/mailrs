export type BlockType = 'text' | 'code' | 'signature' | 'quote' | 'divider' | 'attachment' | 'task'

export type TextBlockData = {
  readonly content: string
  readonly html: string
  readonly format: 'rich' | 'markdown'
}

export type CodeBlockData = {
  readonly code: string
  readonly language: string
}

export type SignatureBlockData = {
  readonly html: string
  readonly text: string
}

export type QuoteBlockData = {
  readonly html: string
  readonly headerHtml: string
  readonly headerText: string
  readonly collapsed: boolean
}

export type DividerBlockData = Record<string, never>

export type AttachmentBlockData = {
  readonly file: File
  readonly name: string
  readonly size: number
  readonly mimeType: string
}

export type TaskBlockData = {
  readonly items: ReadonlyArray<{ readonly id: string; readonly text: string; readonly checked: boolean }>
}

export type BlockDataMap = {
  text: TextBlockData
  code: CodeBlockData
  signature: SignatureBlockData
  quote: QuoteBlockData
  divider: DividerBlockData
  attachment: AttachmentBlockData
  task: TaskBlockData
}

export type Block<T extends BlockType = BlockType> = {
  readonly id: string
  readonly type: T
  readonly data: BlockDataMap[T]
}

export type AnyBlock = Block<BlockType>

export type AssembledEmail = {
  readonly text: string
  readonly html: string
  readonly attachments: ReadonlyArray<File>
}

export function createBlock<T extends BlockType>(type: T, data: BlockDataMap[T]): Block<T> {
  return { id: crypto.randomUUID(), type, data }
}
