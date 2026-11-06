import type { SectionsKey } from './en'

import { word } from '@/i18n/wording'

import { teamZh } from '../team/zh'

export const sectionsZh: Record<SectionsKey, string> = {
  ...teamZh,
  'list.awaiting': word('zh', 'list.awaiting'),
  'list.awaiting.empty': '没有等待回复的会话',
  'sections.accept': '全部接受',
  'sections.accepted': '已接受 {count} 个发件人',
  'sections.acceptFailed': '没能接受这些发件人',
  'sections.bar': '收件箱分区',
  'sections.feed': '通知与推广',
  'sections.followup': '该跟进了',
  'sections.followup.empty': '没有等待超过 {days} 天的会话',
  'sections.new': word('zh', 'sections.new'),
  'sections.new.empty': '没有新发件人的来信',
  'sections.other': '其他',
  'sections.other.empty': '没有其他会话',
  'sections.reply': word('zh', 'sections.reply'),
  'sections.reply.empty': '没有等你回复的信',
  'settings.sections.days': '超过几天提醒跟进',
  'settings.sections.daysUnit': '天',
  'settings.sections.help':
    '把收件箱分成要回复的信、该跟进的会话、新发件人的来信和其他。等对方回复指最后一封是你写的、之后没有真人回信；自动回复不算',
  'settings.sections.toggle': word('zh', 'settings.sections.toggle'),
}
