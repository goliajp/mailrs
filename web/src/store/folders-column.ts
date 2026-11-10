import { atom } from 'jotai'

/**
 * Whether the mail page's folder column is open, kept per browser. It only
 * shows from xl; below that the lists keep their row of tab chips.
 */

const KEY = 'mailrs_folders_column'

const baseFoldersOpenAtom = atom<boolean>(localStorage.getItem(KEY) !== 'closed')

export const foldersOpenAtom = atom(
  (get) => get(baseFoldersOpenAtom),
  (_get, set, open: boolean) => {
    localStorage.setItem(KEY, foldersState(open))
    set(baseFoldersOpenAtom, open)
  }
)

/** The `data-folders` value the `folders:` variants read. */
export function foldersState(open: boolean): 'closed' | 'open' {
  if (open) return 'open'
  return 'closed'
}
