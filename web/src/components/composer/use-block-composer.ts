import type { AnyBlock, BlockDataMap, BlockType } from './types'
import type { AssembledEmail } from './types'

import { useCallback, useEffect, useRef, useState } from 'react'

import { assembleEmail } from './assembly-engine'
import { createBlock } from './types'

type Options = {
  mode: 'forward' | 'new' | 'reply'
  quotedHeader?: string
  quotedHeaderHtml?: string
  quotedHtml?: string
  signature?: string
  signatureEnabled?: boolean
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
      attachment: () => {
        throw new Error('use addAttachment')
      },
      code: () => createBlock('code', { code: '', language: 'javascript' }),
      divider: () => createBlock('divider', {}),
      quote: () =>
        createBlock('quote', {
          collapsed: false,
          headerHtml: '',
          headerText: '',
          html: '',
        }),
      signature: () => createBlock('signature', { html: '', text: '' }),
      task: () =>
        createBlock('task', {
          items: [{ checked: false, id: crypto.randomUUID(), text: '' }],
        }),
      text: () => createBlock('text', { content: '', format: 'markdown', html: '' }),
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
      mimeType: file.type,
      name: file.name,
      size: file.size,
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
    addAttachment,
    addBlock,
    blocks,
    clearCompose,
    getAssembled,
    moveBlock,
    removeBlock,
    updateBlock,
  }
}

function buildInitialBlocks(options: Options): AnyBlock[] {
  const blocks: AnyBlock[] = [createBlock('text', { content: '', format: 'markdown', html: '' })]

  if (options.signatureEnabled && options.signature?.trim()) {
    blocks.push(
      createBlock('signature', {
        html: `<p>${options.signature
          .split('\n')
          .map((l) => l || '<br>')
          .join('</p><p>')}</p>`,
        text: options.signature,
      })
    )
  }

  if (options.mode !== 'new' && options.quotedHtml) {
    blocks.push(
      createBlock('quote', {
        collapsed: true,
        headerHtml: options.quotedHeaderHtml ?? '',
        headerText: options.quotedHeader ?? '',
        html: options.quotedHtml,
      })
    )
  }

  return blocks
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
