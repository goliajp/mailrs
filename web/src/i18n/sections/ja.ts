import type { SectionsKey } from './en'

export const sectionsJa: Record<SectionsKey, string> = {
  'list.awaiting': '返信待ち',
  'list.awaiting.empty': '返信を待っている会話はありません',
  'sections.accept': 'すべて承認',
  'sections.accepted': '{count} 件の送信者を承認しました',
  'sections.acceptFailed': '送信者を承認できませんでした',
  'sections.bar': '受信トレイの区分',
  'sections.feed': '通知とプロモーション',
  'sections.followup': 'フォローアップ',
  'sections.followup.empty': '{days} 日を超えて待っている会話はありません',
  'sections.new': '新しい送信者',
  'sections.new.empty': '新しい送信者からのメールはありません',
  'sections.other': 'その他',
  'sections.other.empty': 'ほかの会話はありません',
  'sections.reply': '要返信',
  'sections.reply.empty': '返信が必要なメールはありません',
  'settings.sections.days': 'フォローアップまでの日数',
  'settings.sections.daysUnit': '日',
  'settings.sections.help':
    '受信トレイを、返信が必要なメール、フォローアップする会話、新しい送信者からのメール、その他に分けます。返信待ちは、最後に自分が送信し、その後に人からの返信がない会話です。自動返信は含みません',
  'settings.sections.toggle': '区分付き受信トレイ',
}
