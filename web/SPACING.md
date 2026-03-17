# Spacing Design Rules

## Core Principle

Spacing is determined by **nesting level**, not by individual judgment.
Each level in the hierarchy has ONE correct spacing value.
If you're unsure, go UP the tree and inherit.

## Spacing Scale

| Token | Value | Use |
|-------|-------|-----|
| `xs`  | 1 (4px)  | icon gaps, inline element spacing |
| `sm`  | 1.5 (6px) | shell gaps, compact controls |
| `md`  | 2 (8px)  | between related elements |
| `lg`  | 3 (12px) | section padding, list items |
| `xl`  | 4 (16px) | panel padding, card padding |
| `2xl` | 6 (24px) | page-level padding |

## Hierarchy Rules

### Level 0: Shell
- Gap between panels: `gap-1.5`
- Shell padding: `pl-1.5 pt-1.5 pb-1.5`
- Status bar: `h-7 px-3`

### Level 1: Panel (rounded-lg bg-raised)
- NO internal padding — content touches edges
- Rounded: `rounded-lg`
- Overflow: `overflow-hidden`

### Level 2: Section headers/bars inside panels
- Horizontal: `px-4`
- Vertical: `py-2`
- Border: `border-b border-[var(--color-border-default)]`
- Between items: `gap-2`

### Level 3: Content areas inside sections
- List items: `px-4 py-3`
- Body text: `px-4 py-3`
- Compose editor content: `px-3 py-2` (tighter for editing)

### Level 4: Inline elements
- Badges: `px-1.5 py-0.5`
- Icon buttons: `p-1` or `p-1.5`
- Text buttons: `px-2 py-1`
- Input fields: `px-3 py-1.5`

## Component-Specific Rules

### Action Bars (bottom bars with buttons)
- Always: `px-4 py-2 border-t`
- Button height: `h-7` (reply) or `h-8` (new conversation)
- Gap: `gap-1.5`

### Dropdown/Popup Menus
- Container: `rounded-lg border shadow-lg py-1`
- Items: `px-3 py-1.5`
- Wide menus: add `p-3` for padded sections

### Tables
- Header: `px-4 py-2`
- Row: `px-4 py-3`

### Cards (bordered containers)
- Standard: `rounded-lg border p-4`
- Compact: `rounded-lg border p-3`

### Forms
- Field stack: `space-y-4`
- Label-to-input: `space-y-1.5`
- Input: `rounded-md border px-3 py-1.5`

## Anti-Patterns (NEVER DO)

1. **Negative positioning that escapes container**: No `-top-N`, `-right-N` on absolutely positioned children
2. **Padding on Panel itself**: Panel is a chrome container — content inside determines its own padding
3. **Mixing px-3 and px-4 in the same section**: Pick one per section level
4. **px-5 or px-6 inside panels**: Only page-level containers (settings, admin) use px-6
5. **Unmatched horizontal padding**: If header is px-4, body must be px-4
