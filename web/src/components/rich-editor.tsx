import { useRef } from 'react'
import { type Editor } from '@tiptap/react'
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
import type { Extensions } from '@tiptap/react'

import { getToken } from '@/store/auth'

const lowlight = createLowlight(common)

// eslint-disable-next-line react-refresh/only-export-components
export async function uploadInlineImage(file: File): Promise<string | null> {
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

// eslint-disable-next-line react-refresh/only-export-components
export function createEditorExtensions(placeholder?: string): Extensions {
  return [
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
  ]
}

// eslint-disable-next-line react-refresh/only-export-components
export function createMinimalExtensions(placeholder?: string): Extensions {
  return [
    StarterKit.configure({
      codeBlock: false,
      link: false,
      underline: false,
      heading: false,
      blockquote: false,
      horizontalRule: false,
    }),
    Link.configure({ openOnClick: false, autolink: true }),
    Placeholder.configure({
      placeholder: placeholder ?? '',
    }),
  ]
}

export const PROSE_CLASS =
  'prose prose-sm max-w-none px-3 py-2 outline-none prose-[var(--color-text-primary)] ' +
  'prose-pre:bg-[#1e1e2e] prose-pre:text-[#cdd6f4] prose-pre:rounded-md ' +
  'prose-code:before:content-none prose-code:after:content-none'

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
      className={`rounded-md px-1.5 py-1 text-xs transition-colors ${
        active
          ? 'bg-[var(--color-border-default)] text-[var(--color-text-primary)]'
          : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-hover)] hover:text-[var(--color-text-secondary)]'
      } disabled:opacity-50`}
    >
      {children}
    </button>
  )
}

// shared: editor toolbar, binds to any editor instance
export function EditorToolbar({ editor }: { editor: Editor | null }) {
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
    <div className="flex shrink-0 flex-wrap items-center gap-0.5 px-2 py-1">
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
        <span className="font-mono text-xs">&lt;/&gt;</span>
      </ToolbarButton>
      <ToolbarButton
        onClick={() => editor.chain().focus().toggleCodeBlock().run()}
        active={editor.isActive('codeBlock')}
        title="Code block"
      >
        <span className="font-mono text-xs">{'{ }'}</span>
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
        <span className="text-xs">Link</span>
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
          <span className="text-xs">Img</span>
        </ToolbarButton>
        <ToolbarButton
          onClick={() => editor.chain().focus().insertTable({ rows: 3, cols: 3 }).run()}
          title="Table"
        >
          <span className="text-xs">Table</span>
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

// eslint-disable-next-line react-refresh/only-export-components
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
