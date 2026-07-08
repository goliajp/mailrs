import type { Extensions } from '@tiptap/react'

import CodeBlockLowlight from '@tiptap/extension-code-block-lowlight'
import Image from '@tiptap/extension-image'
import Link from '@tiptap/extension-link'
import Placeholder from '@tiptap/extension-placeholder'
import { Table } from '@tiptap/extension-table'
import TableCell from '@tiptap/extension-table-cell'
import TableHeader from '@tiptap/extension-table-header'
import TableRow from '@tiptap/extension-table-row'
import TaskItem from '@tiptap/extension-task-item'
import TaskList from '@tiptap/extension-task-list'
import Underline from '@tiptap/extension-underline'
import { type Editor } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import { common, createLowlight } from 'lowlight'
import { useRef } from 'react'

import { wireUploadInlineImage } from '@/wire/endpoints/mail'

const lowlight = createLowlight(common)

// eslint-disable-next-line react-refresh/only-export-components
export function createEditorExtensions(placeholder?: string): Extensions {
  return [
    StarterKit.configure({
      codeBlock: false,
      link: false,
      underline: false,
    }),
    CodeBlockLowlight.configure({
      defaultLanguage: 'plaintext',
      lowlight,
    }),
    Image.configure({
      allowBase64: true,
      inline: true,
    }),
    Link.configure({
      autolink: true,
      openOnClick: false,
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

// eslint-disable-next-line react-refresh/only-export-components
export async function uploadInlineImage(file: File): Promise<null | string> {
  try {
    const data = await wireUploadInlineImage(file)
    if (data.success && data.url) return data.url
  } catch {
    // fallback handled by caller
  }
  return null
}

export const PROSE_CLASS =
  'prose prose-sm max-w-none px-3 py-2 outline-none prose-fg ' +
  'prose-pre:bg-[#1e1e2e] prose-pre:text-[#cdd6f4] prose-pre:rounded-md ' +
  'prose-code:before:content-none prose-code:after:content-none'

type ToolbarButtonProps = {
  active?: boolean
  children: React.ReactNode
  disabled?: boolean
  onClick: () => void
  title: string
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
        active={editor.isActive('bold')}
        onClick={() => editor.chain().focus().toggleBold().run()}
        title="Bold (Ctrl+B)"
      >
        <span className="font-bold">B</span>
      </ToolbarButton>
      <ToolbarButton
        active={editor.isActive('italic')}
        onClick={() => editor.chain().focus().toggleItalic().run()}
        title="Italic (Ctrl+I)"
      >
        <span className="italic">I</span>
      </ToolbarButton>
      <ToolbarButton
        active={editor.isActive('underline')}
        onClick={() => editor.chain().focus().toggleUnderline().run()}
        title="Underline (Ctrl+U)"
      >
        <span className="underline">U</span>
      </ToolbarButton>
      <ToolbarButton
        active={editor.isActive('strike')}
        onClick={() => editor.chain().focus().toggleStrike().run()}
        title="Strikethrough"
      >
        <span className="line-through">S</span>
      </ToolbarButton>

      <div className="bg-border mx-1 h-4 w-px" />

      <ToolbarButton
        active={editor.isActive('code')}
        onClick={() => editor.chain().focus().toggleCode().run()}
        title="Inline code"
      >
        <span className="font-mono text-xs">&lt;/&gt;</span>
      </ToolbarButton>
      <ToolbarButton
        active={editor.isActive('codeBlock')}
        onClick={() => editor.chain().focus().toggleCodeBlock().run()}
        title="Code block"
      >
        <span className="font-mono text-xs">{'{ }'}</span>
      </ToolbarButton>

      <div className="bg-border mx-1 h-4 w-px" />

      <ToolbarButton
        active={editor.isActive('heading', { level: 2 })}
        onClick={() => editor.chain().focus().toggleHeading({ level: 2 }).run()}
        title="Heading"
      >
        H
      </ToolbarButton>
      <ToolbarButton
        active={editor.isActive('blockquote')}
        onClick={() => editor.chain().focus().toggleBlockquote().run()}
        title="Quote"
      >
        &ldquo;
      </ToolbarButton>
      <ToolbarButton
        active={editor.isActive('bulletList')}
        onClick={() => editor.chain().focus().toggleBulletList().run()}
        title="Bullet list"
      >
        &bull;
      </ToolbarButton>
      <ToolbarButton
        active={editor.isActive('orderedList')}
        onClick={() => editor.chain().focus().toggleOrderedList().run()}
        title="Numbered list"
      >
        1.
      </ToolbarButton>
      <ToolbarButton active={editor.isActive('link')} onClick={addLink} title="Link">
        <span className="text-xs">Link</span>
      </ToolbarButton>

      {/* secondary tools — hidden on narrow screens */}
      <div className="bg-border mx-1 hidden h-4 w-px sm:block" />
      <input
        accept="image/*"
        className="hidden"
        onChange={handleImageFile}
        ref={fileInputRef}
        type="file"
      />
      <div className="hidden items-center gap-0.5 sm:flex">
        <ToolbarButton
          active={editor.isActive('taskList')}
          onClick={() => editor.chain().focus().toggleTaskList().run()}
          title="Task list"
        >
          &#9744;
        </ToolbarButton>
        <ToolbarButton onClick={addImage} title="Image">
          <span className="text-xs">Img</span>
        </ToolbarButton>
        <ToolbarButton
          onClick={() => editor.chain().focus().insertTable({ cols: 3, rows: 3 }).run()}
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
  html: string
  text: string
} {
  if (!editor) return { html: '', text: '' }
  return {
    html: editor.getHTML(),
    text: editor.getText(),
  }
}

function ToolbarButton({ active, children, disabled, onClick, title }: ToolbarButtonProps) {
  return (
    <button
      aria-label={title}
      aria-pressed={active}
      className={`rounded-md px-1.5 py-1 text-xs transition-colors ${
        active ? 'bg-border text-fg' : 'text-fg-muted hover:bg-bg-secondary hover:text-fg-secondary'
      } disabled:opacity-50`}
      disabled={disabled}
      onClick={onClick}
      title={title}
      type="button"
    >
      {children}
    </button>
  )
}
