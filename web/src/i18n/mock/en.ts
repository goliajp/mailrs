/** Mock-mail entries, spread into `en`; kept apart to keep each file small. */
export const mockEn = {
  'mock.address': 'Address',
  'mock.dir.capturedOut': 'Sent by this sandbox and kept here instead of being delivered',
  'mock.dir.in': 'Received at a mock address',
  'mock.disabled.description':
    'This server has no mock mail domain configured, so there are no test mailboxes to show',
  'mock.disabled.title': 'Mock mail is not enabled on this deployment',
  'mock.list.dropped': '{count} older messages were dropped to stay under the namespace cap',
  'mock.list.empty': 'No mail yet. This list refreshes every 5 seconds',
  'mock.list.label': 'Messages',
  'mock.mint.failed': 'Could not mint an address',
  'mock.mint.new': 'New address',
  'mock.mint.tag': 'Tag (optional)',
  'mock.mint.tagPlaceholder': 'tag (optional), e.g. signup-01',
  'mock.noneAnyone.description': 'Namespaces anyone creates are listed here',
  'mock.noneAnyone.title': 'Nobody has created a namespace yet',
  'mock.noneMine.description':
    'Name one on the left; every address under it is a test mailbox you can read here',
  'mock.noneMine.title': 'Create a namespace to get test addresses',
  'mock.ns.createFailed': 'Could not create the namespace',
  'mock.ns.delete': 'Delete namespace {name}',
  'mock.ns.deleteConfirm':
    'Delete namespace {name} and every message in it? Its addresses stop receiving mail.',
  'mock.ns.deleted': 'Deleted {name}',
  'mock.ns.deleteFailed': 'Could not delete the namespace',
  'mock.ns.list': 'Namespaces',
  'mock.ns.nameRule': '3–32 characters of a-z, 0-9 and -, unique across the server',
  'mock.ns.newName': 'New namespace name',
  'mock.ns.stats': '{messages} messages, {bytes}, {dropped} dropped',
  'mock.pick.description':
    'Its test addresses, the mail they received and the sandbox tools show here',
  'mock.pick.title': 'Pick a namespace',
  'mock.scope': 'Whose namespaces',
  'mock.scope.everyone': 'Everyone’s',
  'mock.scope.mine': 'Mine',
  'mock.subtitle': 'Disposable mailboxes for tests: sign up with an address, read the code here',
  'mock.tool.send': 'Send test mail',
  'mock.tool.smtp': 'SMTP sandbox',
  'mock.tool.webhook': 'Webhook',
} as const

export type MockKey = keyof typeof mockEn
