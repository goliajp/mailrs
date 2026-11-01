import type { MockKey } from './en'

export const mockJa: Record<MockKey, string> = {
  'mock.address': 'アドレス',
  'mock.dir.capturedOut': 'このサンドボックスから送信され、配信されずにここに保管されたもの',
  'mock.dir.in': 'モックアドレスで受信したもの',
  'mock.disabled.description':
    'このサーバーにはモックメールのドメインが設定されていないため、表示できるテスト用メールボックスはありません',
  'mock.disabled.title': 'このデプロイではモックメールが有効になっていません',
  'mock.list.dropped':
    'ネームスペースの上限を超えないよう、古いメッセージ {count} 件を破棄しました',
  'mock.list.empty': 'まだメールはありません。この一覧は 5 秒ごとに更新されます',
  'mock.list.label': 'メッセージ',
  'mock.mint.failed': 'アドレスを発行できませんでした',
  'mock.mint.new': '新しいアドレス',
  'mock.mint.tag': 'タグ（任意）',
  'mock.mint.tagPlaceholder': 'タグ（任意）、例: signup-01',
  'mock.noneAnyone.description': '誰かが作成したネームスペースはここに表示されます',
  'mock.noneAnyone.title': 'まだ誰もネームスペースを作成していません',
  'mock.noneMine.description':
    '左側で名前を付けてください。その下のアドレスはすべて、ここで読めるテスト用メールボックスになります',
  'mock.noneMine.title': 'ネームスペースを作成してテスト用アドレスを取得',
  'mock.ns.createFailed': 'ネームスペースを作成できませんでした',
  'mock.ns.delete': 'ネームスペース {name} を削除',
  'mock.ns.deleteConfirm':
    'ネームスペース {name} とその中のすべてのメッセージを削除しますか？そのアドレスはメールを受信しなくなります。',
  'mock.ns.deleted': '{name} を削除しました',
  'mock.ns.deleteFailed': 'ネームスペースを削除できませんでした',
  'mock.ns.list': 'ネームスペース',
  'mock.ns.nameRule': 'a-z、0-9、- の 3〜32 文字で、サーバー内で重複できません',
  'mock.ns.newName': '新しいネームスペース名',
  'mock.ns.stats': '{messages} 件、{bytes}、破棄 {dropped} 件',
  'mock.pick.description':
    'そのテスト用アドレス、受信したメール、サンドボックスのツールがここに表示されます',
  'mock.pick.title': 'ネームスペースを選択してください',
  'mock.scope': '誰のネームスペース',
  'mock.scope.everyone': '全員の',
  'mock.scope.mine': '自分の',
  'mock.subtitle':
    'テスト用の使い捨てメールボックス。アドレスで登録して、届いたコードをここで確認できます',
  'mock.tool.send': 'テストメールを送信',
  'mock.tool.smtp': 'SMTP サンドボックス',
  'mock.tool.webhook': 'Webhook',
}
