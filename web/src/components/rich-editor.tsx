import { useCallback, useEffect, useRef, useState } from 'react'
import { useEditor, EditorContent, type Editor } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import CodeBlockLowlight from '@tiptap/extension-code-block-lowlight'
import Image from '@tiptap/extension-image'
import Link from '@tiptap/extension-link'
import { Table } from '@tiptap/extension-table'
import TableRow from '@tiptap/extension-table-row'
import TableCell from '@tiptap/extension-table-cell'
import TableHeader from '@tiptap/extension-table-header'
import TaskList from '@tiptap/extension-task-list'
import TaskItem from '@tiptap/extension-task-item'
import Placeholder from '@tiptap/extension-placeholder'
import Underline from '@tiptap/extension-underline'
import { common, createLowlight } from 'lowlight'

import { getToken } from '@/store/auth'

const lowlight = createLowlight(common)

async function uploadInlineImage(file: File): Promise<string | null> {
  const form = new FormData()
  form.append('image', file)
  const token = getToken()
  const headers: Record<string, string> = {}
  if (token) headers['Authorization'] = `Bearer ${token}`
  try {
    const res = await fetch('/api/mail/inline-upload', {
      method: 'POST',
      headers,
      body: form,
    })
    const data = await res.json()
    if (data.success && data.url) return data.url as string
  } catch {
    // fallback handled by caller
  }
  return null
}

type ToolbarButtonProps = {
  onClick: () => void
  active?: boolean
  disabled?: boolean
  title: string
  children: React.ReactNode
}

function ToolbarButton({ onClick, active, disabled, title, children }: ToolbarButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      className={`rounded-md px-1.5 py-0.5 text-xs transition-colors ${
        active
          ? 'bg-[var(--color-border-default)] text-[var(--color-text-primary)]'
          : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]'
      } disabled:opacity-50`}
    >
      {children}
    </button>
  )
}

function Toolbar({ editor }: { editor: ReturnType<typeof useEditor> }) {
  const fileInputRef = useRef<HTMLInputElement>(null)

  if (!editor) return null

  const addLink = () => {
    const url = window.prompt('URL')
    if (url) {
      editor.chain().focus().extendMarkRange('link').setLink({ href: url }).run()
    }
  }
  const addImage = () => {
    fileInputRef.current?.click()
  }
  const handleImageFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file || !editor) return
    const url = await uploadInlineImage(file)
    if (url) {
      editor.chain().focus().setImage({ src: url }).run()
    }
    e.target.value = ''
  }

  return (
    <div className="flex shrink-0 flex-wrap items-center gap-0.5 border-b border-[var(--color-border-default)] px-2 py-1">
      <ToolbarButton
        onClick={() => editor.chain().focus().toggleBold().run()}
        active={editor.isActive('bold')}
        title="Bold (Ctrl+B)"
      >
        <span className="font-bold">B</span>
      </ToolbarButton>
      <ToolbarButton
        onClick={() => editor.chain().focus().toggleItalic().run()}
        active={editor.isActive('italic')}
        title="Italic (Ctrl+I)"
      >
        <span className="italic">I</span>
      </ToolbarButton>
      <ToolbarButton
        onClick={() => editor.chain().focus().toggleUnderline().run()}
        active={editor.isActive('underline')}
        title="Underline (Ctrl+U)"
      >
        <span className="underline">U</span>
      </ToolbarButton>
      <ToolbarButton
        onClick={() => editor.chain().focus().toggleStrike().run()}
        active={editor.isActive('strike')}
        title="Strikethrough"
      >
        <span className="line-through">S</span>
      </ToolbarButton>

      <div className="mx-1 h-4 w-px bg-[var(--color-border-default)]" />

      <ToolbarButton
        onClick={() => editor.chain().focus().toggleCode().run()}
        active={editor.isActive('code')}
        title="Inline code"
      >
        <span className="font-mono text-[10px]">&lt;/&gt;</span>
      </ToolbarButton>
      <ToolbarButton
        onClick={() => editor.chain().focus().toggleCodeBlock().run()}
        active={editor.isActive('codeBlock')}
        title="Code block"
      >
        <span className="font-mono text-[10px]">{'{ }'}</span>
      </ToolbarButton>

      <div className="mx-1 h-4 w-px bg-[var(--color-border-default)]" />

      <ToolbarButton
        onClick={() => editor.chain().focus().toggleHeading({ level: 2 }).run()}
        active={editor.isActive('heading', { level: 2 })}
        title="Heading"
      >
        H
      </ToolbarButton>
      <ToolbarButton
        onClick={() => editor.chain().focus().toggleBlockquote().run()}
        active={editor.isActive('blockquote')}
        title="Quote"
      >
        &ldquo;
      </ToolbarButton>
      <ToolbarButton
        onClick={() => editor.chain().focus().toggleBulletList().run()}
        active={editor.isActive('bulletList')}
        title="Bullet list"
      >
        &bull;
      </ToolbarButton>
      <ToolbarButton
        onClick={() => editor.chain().focus().toggleOrderedList().run()}
        active={editor.isActive('orderedList')}
        title="Numbered list"
      >
        1.
      </ToolbarButton>
      <ToolbarButton onClick={addLink} active={editor.isActive('link')} title="Link">
        <span className="text-[10px]">Link</span>
      </ToolbarButton>

      {/* secondary tools — hidden on narrow screens */}
      <div className="mx-1 hidden h-4 w-px bg-[var(--color-border-default)] sm:block" />
      <input
        ref={fileInputRef}
        type="file"
        accept="image/*"
        className="hidden"
        onChange={handleImageFile}
      />
      <div className="hidden items-center gap-0.5 sm:flex">
        <ToolbarButton
          onClick={() => editor.chain().focus().toggleTaskList().run()}
          active={editor.isActive('taskList')}
          title="Task list"
        >
          &#9744;
        </ToolbarButton>
        <ToolbarButton onClick={addImage} title="Image">
          <span className="text-[10px]">Img</span>
        </ToolbarButton>
        <ToolbarButton
          onClick={() => editor.chain().focus().insertTable({ rows: 3, cols: 3 }).run()}
          title="Table"
        >
          <span className="text-[10px]">Table</span>
        </ToolbarButton>
        <ToolbarButton
          onClick={() => editor.chain().focus().setHorizontalRule().run()}
          title="Divider"
        >
          &mdash;
        </ToolbarButton>
      </div>
    </div>
  )
}

export function RichEditor({
  onSubmit,
  placeholder,
  disabled,
  minHeight,
  getEditorRef,
}: {
  onSubmit: () => void
  placeholder?: string
  disabled?: boolean
  minHeight?: string
  getEditorRef?: (editor: Editor | null) => void
}) {
  const [isDragOver, setIsDragOver] = useState(false)
  const dragCountRef = useRef(0)

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        codeBlock: false,
        link: false,
        underline: false,
      }),
      CodeBlockLowlight.configure({
        lowlight,
        defaultLanguage: 'plaintext',
      }),
      Image.configure({
        inline: true,
        allowBase64: true,
      }),
      Link.configure({
        openOnClick: false,
        autolink: true,
      }),
      Table.configure({ resizable: false }),
      TableRow,
      TableCell,
      TableHeader,
      TaskList,
      TaskItem.configure({ nested: true }),
      Placeholder.configure({
        placeholder: placeholder ?? 'Write your message...',
      }),
      Underline,
    ],
    editorProps: {
      attributes: {
        class:
          'prose prose-sm max-w-none px-3 py-2 outline-none prose-[var(--color-text-primary)] ' +
          'prose-pre:bg-[#1e1e2e] prose-pre:text-[#cdd6f4] prose-pre:rounded-md ' +
          'prose-code:before:content-none prose-code:after:content-none ' +
          'min-h-[' + (minHeight ?? '3rem') + ']',
      },
      handleKeyDown: (_view, event) => {
        if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
          event.preventDefault()
          onSubmit()
          return true
        }
        if (event.key === 'Tab' && editor?.isActive('codeBlock')) {
          event.preventDefault()
          if (event.shiftKey) {
            return true
          }
          editor?.commands.insertContent('  ')
          return true
        }
        return false
      },
    },
    editable: !disabled,
  })

  useEffect(() => {
    if (editor && getEditorRef) {
      getEditorRef(editor)
    }
  }, [editor, getEditorRef])

  const handleDrop = useCallback(
    async (e: React.DragEvent) => {
      setIsDragOver(false)
      dragCountRef.current = 0
      if (!editor) return
      const files = Array.from(e.dataTransfer.files).filter((f) => f.type.startsWith('image/'))
      if (files.length === 0) return
      e.preventDefault()
      for (const file of files) {
        const url = await uploadInlineImage(file)
        if (url) {
          editor.chain().focus().setImage({ src: url }).run()
        }
      }
    },
    [editor],
  )

  const handlePaste = useCallback(
    async (e: React.ClipboardEvent) => {
      if (!editor) return
      const items = Array.from(e.clipboardData.items).filter((i) => i.type.startsWith('image/'))
      if (items.length === 0) return
      e.preventDefault()
      for (const item of items) {
        const file = item.getAsFile()
        if (!file) continue
        const url = await uploadInlineImage(file)
        if (url) {
          editor.chain().focus().setImage({ src: url }).run()
        }
      }
    },
    [editor],
  )

  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    dragCountRef.current += 1
    if (dragCountRef.current === 1) setIsDragOver(true)
  }, [])

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    dragCountRef.current -= 1
    if (dragCountRef.current === 0) setIsDragOver(false)
  }, [])

  return (
    <div
      className={`relative flex h-full flex-col rounded-lg border transition-colors ${
        isDragOver
          ? 'border-[var(--color-brand-primary)] bg-[var(--color-brand-subtle)]'
          : 'border-[var(--color-border-default)] bg-[var(--color-bg-sunken)]'
      }`}
      onDrop={handleDrop}
      onPaste={handlePaste}
      onDragOver={(e) => e.preventDefault()}
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
    >
      <Toolbar editor={editor} />
      <div className={`min-h-0 flex-1 overflow-y-auto ${disabled ? 'pointer-events-none opacity-50' : ''}`}>
        <EditorContent editor={editor} />
      </div>
      {isDragOver && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
          <span className="rounded-full bg-[var(--color-brand-primary)] px-3 py-1 text-xs font-medium text-white shadow-lg">
            Drop image to insert
          </span>
        </div>
      )}
    </div>
  )
}

// utility to get plain text and html from editor
export function getEditorContent(editor: Editor | null): {
  text: string
  html: string
} {
  if (!editor) return { text: '', html: '' }
  return {
    text: editor.getText(),
    html: editor.getHTML(),
  }
}
