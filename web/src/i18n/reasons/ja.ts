import type { ReasonsKey } from './en'

import { word } from '@/i18n/wording'

export const reasonsJa: Record<ReasonsKey, string> = {
  'fraud.examined': '確認済み: 不審な点があります',
  'fraud.kept': '削除せずに保管しています。不正利用の報告に使う証拠です',
  'fraud.layer.clean': '問題は見つかりませんでした',
  'fraud.layer.content': '内容',
  'fraud.layer.content.question': '詐欺らしい内容か',
  'fraud.layer.identity': '身元',
  'fraud.layer.identity.question': '表示している名前を名乗れる送信者か',
  'fraud.layer.provenance': '出所',
  'fraud.layer.provenance.question': '実在のメールソフトが書いたものか',
  'fraud.layer.transport': '経路',
  'fraud.layer.transport.question': '名乗っている場所から届いたか',
  'fraud.notFraud': 'これは詐欺ではありません',
  'fraud.placement': 'ここにある理由',
  'fraud.releasing': '解除しています…',
  'fraud.rules': 'ルール {version}',
  'link.warning.confirm':
    'このリンクは見た目どおりの場所に行かない可能性があります。\n\n{reasons}\n\nそれでも開きますか？',
  'reason.class.inbox': word('ja', 'reason.class.inbox'),
  'reason.class.notification': '通知',
  'reason.class.promotion': 'プロモーション',
}
