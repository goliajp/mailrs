import type { ReasonsKey } from './en'

export const reasonsZh: Record<ReasonsKey, string> = {
  'fraud.examined': '已检查：有可疑之处',
  'fraud.kept': '保留、不删除：举报滥用时要用它作证据。',
  'fraud.layer.clean': '没有发现问题。',
  'fraud.layer.content': '内容',
  'fraud.layer.content.question': '内容像不像诈骗？',
  'fraud.layer.identity': '身份',
  'fraud.layer.identity.question': '它显示的名字是否属实？',
  'fraud.layer.provenance': '来源',
  'fraud.layer.provenance.question': '是不是真实的邮件客户端写的？',
  'fraud.layer.transport': '传输',
  'fraud.layer.transport.question': '是不是从它声称的地方发来的？',
  'fraud.notFraud': '这不是诈骗',
  'fraud.placement': '为什么在这里',
  'fraud.releasing': '正在放行…',
  'fraud.rules': '规则 {version}',
  'link.warning.confirm': '这个链接可能不是它看上去的地方。\n\n{reasons}\n\n仍然打开吗？',
  'reason.class.inbox': '收件箱',
  'reason.class.notification': '通知',
  'reason.class.promotion': '推广',
}
