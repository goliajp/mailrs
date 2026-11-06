import type { SectionsKey } from './en'

import { word } from '@/i18n/wording'

import { teamJa } from '../team/ja'

export const sectionsJa: Record<SectionsKey, string> = {
  ...teamJa,
  'list.awaiting': word('ja', 'list.awaiting'),
  'list.awaiting.empty': '返信を待っている会話はありません',
  'sections.accept': 'すべて承認',
  'sections.accepted': '{count} 件の送信者を承認しました',
  'sections.acceptFailed': '送信者を承認できませんでした',
  'sections.bar': '受信トレイの区分',
  'sections.feed': '通知とプロモーション',
  'sections.followup': 'フォローアップ',
  'sections.followup.empty': '{days} 日を超えて待っている会話はありません',
  'sections.new': word('ja', 'sections.new'),
  'sections.new.empty': '新しい送信者からのメールはありません',
  'sections.other': 'その他',
  'sections.other.empty': 'ほかの会話はありません',
  'sections.reply': word('ja', 'sections.reply'),
  'sections.reply.empty': '返信が必要なメールはありません',
  'settings.sections.days': 'フォローアップまでの日数',
  'settings.sections.daysUnit': '日',
  'settings.sections.help':
    '受信トレイを、返信が必要なメール、フォローアップする会話、新しい送信者からのメール、その他に分けます。返信待ちは、最後に自分が送信し、その後に人からの返信がない会話です。自動返信は含みません',
  'settings.sections.toggle': word('ja', 'settings.sections.toggle'),
}
