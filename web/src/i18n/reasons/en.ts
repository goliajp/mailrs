import { word } from '@/i18n/wording'

/** Why a message was judged as it was: the verdict panel and link warnings. */
export const reasonsEn = {
  'fraud.examined': 'Examined: something looked wrong',
  'fraud.kept': 'Kept, not deleted — this is the evidence an abuse report is built from.',
  'fraud.layer.clean': 'Nothing found.',
  'fraud.layer.content': 'Content',
  'fraud.layer.content.question': 'Does it read like the fraud it is?',
  'fraud.layer.identity': 'Identity',
  'fraud.layer.identity.question': 'Is the name it shows a claim it can make?',
  'fraud.layer.provenance': 'Provenance',
  'fraud.layer.provenance.question': 'Did a real mail client write it?',
  'fraud.layer.transport': 'Transport',
  'fraud.layer.transport.question': 'Did it come from where it says?',
  'fraud.notFraud': 'This is not fraud',
  'fraud.placement': 'Why it is here',
  'fraud.releasing': 'Releasing…',
  'fraud.rules': 'rules {version}',
  'link.warning.confirm': 'This link may not go where it seems.\n\n{reasons}\n\nOpen it anyway?',
  'reason.class.inbox': word('en', 'reason.class.inbox'),
  'reason.class.notification': 'Notifications',
  'reason.class.promotion': 'Promotions',
} as const

export type ReasonsKey = keyof typeof reasonsEn
