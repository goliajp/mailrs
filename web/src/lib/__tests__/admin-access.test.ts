import { describe, expect, it } from 'vitest'

import { ADMIN_PAGES } from '@/components/admin-nav'

import { canOpenAdminPage, PAGE_NEED, showsAdmin } from '../admin-access'

const plain = { is_admin: false, is_super: false, permissions: [] }
const aliasesOnly = { is_admin: true, is_super: false, permissions: ['admin.aliases'] }
const root = { is_admin: true, is_super: true, permissions: [] }

describe('admin access', () => {
  it('shows the admin entry to anyone with an admin permission, not only to super administrators', () => {
    expect(showsAdmin(plain)).toBe(false)
    expect(showsAdmin(aliasesOnly)).toBe(true)
    expect(showsAdmin(root)).toBe(true)
    expect(showsAdmin(null)).toBe(false)
  })

  it('offers a partial administrator the pages their permission opens, and the overview', () => {
    expect(canOpenAdminPage(aliasesOnly, '/admin/aliases')).toBe(true)
    expect(canOpenAdminPage(aliasesOnly, '/admin/overview')).toBe(true)
    expect(canOpenAdminPage(aliasesOnly, '/admin/accounts')).toBe(false)
    expect(canOpenAdminPage(aliasesOnly, '/admin/queues')).toBe(false)
  })

  it('treats a page missing from the table as super-only, like the server', () => {
    expect(canOpenAdminPage(aliasesOnly, '/admin/not-yet-listed')).toBe(false)
    expect(canOpenAdminPage(root, '/admin/not-yet-listed')).toBe(true)
  })

  it('has a permission for every page the admin sidebar offers', () => {
    expect(ADMIN_PAGES.filter((p) => !(p in PAGE_NEED))).toEqual([])
  })
})
