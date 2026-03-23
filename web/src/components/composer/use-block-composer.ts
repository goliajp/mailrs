import { useState, useCallback, useEffect, useRef } from 'react'
import type { AnyBlock, BlockType, BlockDataMap } from './types'
import { createBlock } from './types'
import { assembleEmail } from './assembly-engine'
import type { AssembledEmail } from './types'

type Options = {
  signature?: string
  signatureEnabled?: boolean
  quotedHtml?: string
  quotedHeader?: string
  quotedHeaderHtml?: string
  mode: 'new' | 'reply' | 'forward'
}

function buildInitialBlocks(options: Options): AnyBlock[] {
  const blocks: AnyBlock[] = [createBlock('text', { content: '', html: '', format: 'rich' })]

  if (options.signatureEnabled && options.signature?.trim()) {
    blocks.push(
      createBlock('signature', {
        html: `<p>${options.signature
          .split('\n')
          .map((l) => l || '<br>')
          .join('</p><p>')}</p>`,
        text: options.signature,
      }),
    )
  }

  if (options.mode !== 'new' && options.quotedHtml) {
    blocks.push(
      createBlock('quote', {
        html: options.quotedHtml,
        headerHtml: options.quotedHeaderHtml ?? '',
        headerText: options.quotedHeader ?? '',
        collapsed: true,
      }),
    )
  }

  return blocks
}

export function useBlockComposer(options: Options) {
  const initializedRef = useRef(false)
  const [blocks, setBlocks] = useState<AnyBlock[]>(() => buildInitialBlocks(options))

  // reinitialize when mode/quoted content changes
  useEffect(() => {
    if (!initializedRef.current) {
      initializedRef.current = true
      return
    }
    setBlocks(buildInitialBlocks(options))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [options.mode, options.quotedHtml])

  const addBlock = useCallback((type: BlockType, position?: number) => {
    const defaults: Record<BlockType, () => AnyBlock> = {
      text: () => createBlock('text', { content: '', html: '', format: 'rich' }),
      code: () => createBlock('code', { code: '', language: 'javascript' }),
      signature: () => createBlock('signature', { html: '', text: '' }),
      quote: () =>
        createBlock('quote', { html: '', headerHtml: '', headerText: '', collapsed: false }),
      divider: () => createBlock('divider', {}),
      attachment: () => {
        throw new Error('use addAttachment')
      },
      task: () =>
        createBlock('task', { items: [{ id: crypto.randomUUID(), text: '', checked: false }] }),
    }
    const block = defaults[type]()
    setBlocks((prev) => {
      const next = [...prev]
      const idx = position ?? findInsertPosition(next)
      next.splice(idx, 0, block)
      return next
    })
    return block.id
  }, [])

  const addAttachment = useCallback((file: File) => {
    const block = createBlock('attachment', {
      file,
      name: file.name,
      size: file.size,
      mimeType: file.type,
    })
    setBlocks((prev) => {
      const next = [...prev]
      const idx = findInsertPosition(next)
      next.splice(idx, 0, block)
      return next
    })
  }, [])

  const removeBlock = useCallback((id: string) => {
    setBlocks((prev) => prev.filter((b) => b.id !== id))
  }, [])

  const updateBlock = useCallback((id: string, data: BlockDataMap[BlockType]) => {
    setBlocks((prev) => prev.map((b) => (b.id === id ? { ...b, data } : b)))
  }, [])

  const moveBlock = useCallback((fromIndex: number, toIndex: number) => {
    setBlocks((prev) => {
      const next = [...prev]
      const [moved] = next.splice(fromIndex, 1)
      next.splice(toIndex, 0, moved)
      return next
    })
  }, [])

  const clearCompose = useCallback(() => {
    setBlocks(buildInitialBlocks(options))
  }, [options])

  const getAssembled = useCallback((): AssembledEmail => {
    return assembleEmail(blocks)
  }, [blocks])

  return {
    blocks,
    addBlock,
    addAttachment,
    removeBlock,
    updateBlock,
    moveBlock,
    clearCompose,
    getAssembled,
  }
}

// find position before signature/quote blocks
function findInsertPosition(blocks: AnyBlock[]): number {
  for (let i = blocks.length - 1; i >= 0; i--) {
    if (blocks[i].type !== 'signature' && blocks[i].type !== 'quote') {
      return i + 1
    }
  }
  return 0
}
