export type AnyBlock = Block<BlockType>

export type AssembledEmail = {
  readonly attachments: ReadonlyArray<File>
  readonly html: string
  readonly text: string
}

export type AttachmentBlockData = {
  readonly file: File
  readonly mimeType: string
  readonly name: string
  readonly size: number
}

export type Block<T extends BlockType = BlockType> = {
  readonly data: BlockDataMap[T]
  readonly id: string
  readonly type: T
}

export type BlockDataMap = {
  attachment: AttachmentBlockData
  code: CodeBlockData
  divider: DividerBlockData
  quote: QuoteBlockData
  signature: SignatureBlockData
  task: TaskBlockData
  text: TextBlockData
}

export type BlockType = 'attachment' | 'code' | 'divider' | 'quote' | 'signature' | 'task' | 'text'

export type CodeBlockData = {
  readonly code: string
  readonly language: string
}

export type DividerBlockData = Record<string, never>

export type QuoteBlockData = {
  readonly collapsed: boolean
  readonly headerHtml: string
  readonly headerText: string
  readonly html: string
}

export type SignatureBlockData = {
  readonly html: string
  readonly text: string
}

export type TaskBlockData = {
  readonly items: ReadonlyArray<{
    readonly checked: boolean
    readonly id: string
    readonly text: string
  }>
}

export type TextBlockData = {
  readonly content: string
  readonly format: 'markdown' | 'rich'
  readonly html: string
}

export function createBlock<T extends BlockType>(type: T, data: BlockDataMap[T]): Block<T> {
  return { data, id: crypto.randomUUID(), type }
}
