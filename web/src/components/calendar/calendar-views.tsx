import type { Key } from '@/i18n'
import type { WireCalendarEvent } from '@/wire/schemas/calendar'

import { cx } from '@goliapkg/gds'
import { useSetAtom } from 'jotai'
import { useNavigate } from 'react-router'

import { useLocale, useT } from '@/i18n'
import { eventsOn, sameDay, spanOf } from '@/lib/calendar-grid'
import { openThreadAtom } from '@/store/ui'

/** Events shown in a month cell before the rest collapse into "+N more". */
const MONTH_CELL_LIMIT = 3

export function EventDetail({ ev, onClose }: { ev: WireCalendarEvent; onClose: () => void }) {
  const t = useT()
  const locale = useLocale()
  const span = spanOf(ev)
  const answer = answerLabel(ev.partstat)
  return (
    <section
      aria-label={ev.summary}
      className="border-border bg-surface rounded-md border p-3 text-sm"
    >
      <div className="flex items-start justify-between gap-2">
        <h2 className={cx('text-fg font-semibold', isStruck(ev) && 'line-through')}>
          {ev.summary}
        </h2>
        <button className="text-fg-secondary hover:text-fg text-xs" onClick={onClose} type="button">
          {t('common.close')}
        </button>
      </div>
      <dl className="text-fg-secondary mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
        {span && (
          <>
            <dt>{t('invite.start')}</dt>
            <dd>{new Date(span.start).toLocaleString(locale)}</dd>
            <dt>{t('invite.end')}</dt>
            <dd>{new Date(span.end).toLocaleString(locale)}</dd>
          </>
        )}
        {ev.organizer && (
          <>
            <dt>{t('calendar.organizer')}</dt>
            <dd>{ev.organizer}</dd>
          </>
        )}
        <dt>{t('calendar.from')}</dt>
        <dd>{t(sourceLabel(ev.source))}</dd>
        {answer && (
          <>
            <dt>{t('calendar.answer')}</dt>
            <dd>{t(answer)}</dd>
          </>
        )}
        {ev.status === 'CANCELLED' && (
          <>
            <dt>{t('calendar.status')}</dt>
            <dd>{t('invite.badge.cancelled')}</dd>
          </>
        )}
      </dl>
      {ev.thread_id && <OpenMail threadId={ev.thread_id} />}
    </section>
  )
}

export function MonthView({
  anchor,
  days,
  events,
  onOpen,
  today,
}: {
  anchor: Date
  days: Date[]
  events: WireCalendarEvent[]
  onOpen: (ev: WireCalendarEvent) => void
  today: Date
}) {
  const t = useT()
  return (
    <div className="grid grid-cols-7 gap-1" role="grid">
      {days.map((day) => {
        const onDay = eventsOn(events, day)
        const hidden = onDay.length - MONTH_CELL_LIMIT
        return (
          <div
            className={cx(
              'border-border min-h-20 rounded-md border p-1',
              day.getMonth() !== anchor.getMonth() && 'opacity-50'
            )}
            key={day.toISOString()}
            role="gridcell"
          >
            <div
              className={cx(
                'text-fg-secondary text-xs tabular-nums',
                sameDay(day, today) && 'text-accent font-semibold'
              )}
            >
              {day.getDate()}
            </div>
            <div className="flex flex-col gap-0.5">
              {onDay.slice(0, MONTH_CELL_LIMIT).map((ev) => (
                <EventChip ev={ev} key={ev.uid} onOpen={onOpen} />
              ))}
              {hidden > 0 && (
                <span className="text-fg-muted text-xs">{t('calendar.more', { n: hidden })}</span>
              )}
            </div>
          </div>
        )
      })}
    </div>
  )
}

export function WeekView({
  days,
  events,
  onOpen,
  today,
}: {
  days: Date[]
  events: WireCalendarEvent[]
  onOpen: (ev: WireCalendarEvent) => void
  today: Date
}) {
  return (
    <div className="grid grid-cols-1 gap-2 sm:grid-cols-7" role="grid">
      {days.map((day) => (
        <div
          className="border-border min-h-24 rounded-md border p-1.5 sm:min-h-64"
          key={day.toISOString()}
          role="gridcell"
        >
          <DayHeading day={day} today={today} />
          <div className="flex flex-col gap-1">
            {eventsOn(events, day).map((ev) => (
              <EventChip ev={ev} key={ev.uid} onOpen={onOpen} />
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}

function answerLabel(partstat: WireCalendarEvent['partstat']): Key | null {
  switch (partstat) {
    case 'ACCEPTED':
      return 'invite.guest.accepted'
    case 'DECLINED':
      return 'invite.guest.declined'
    case 'TENTATIVE':
      return 'invite.guest.tentative'
    default:
      return null
  }
}

function DayHeading({ day, today }: { day: Date; today: Date }) {
  const locale = useLocale()
  return (
    <div
      className={cx(
        'text-fg-secondary mb-1 text-xs',
        sameDay(day, today) && 'text-accent font-semibold'
      )}
    >
      {day.toLocaleDateString(locale, { day: 'numeric', month: 'numeric', weekday: 'short' })}
    </div>
  )
}

function EventChip({
  ev,
  onOpen,
}: {
  ev: WireCalendarEvent
  onOpen: (ev: WireCalendarEvent) => void
}) {
  const locale = useLocale()
  return (
    <button
      className={cx(
        'bg-accent/10 text-fg hover:bg-accent/20 w-full truncate rounded px-1.5 py-0.5 text-left text-xs',
        isStruck(ev) && 'text-fg-muted line-through'
      )}
      data-testid="calendar-event"
      onClick={() => onOpen(ev)}
      title={ev.summary}
      type="button"
    >
      <span className="text-fg-muted mr-1 tabular-nums">{timeOf(ev, locale)}</span>
      {ev.summary}
    </button>
  )
}

/** A meeting the reader declined or the organiser cancelled is drawn struck through. */
function isStruck(ev: WireCalendarEvent): boolean {
  return ev.partstat === 'DECLINED' || ev.status === 'CANCELLED'
}

/** The mail the invitation came in, opened in the mail view. */
function OpenMail({ threadId }: { threadId: string }) {
  const t = useT()
  const openThread = useSetAtom(openThreadAtom)
  const navigate = useNavigate()
  return (
    <button
      className="text-accent mt-3 text-xs hover:underline"
      onClick={() => {
        openThread({ list: 'inbox', threadId, uid: null })
        navigate('/mail')
      }}
      type="button"
    >
      {t('calendar.openMail')}
    </button>
  )
}

function sourceLabel(source: null | string): Key {
  if (source?.startsWith('feed:')) return 'calendar.source.feed'
  if (source?.startsWith('mail:')) return 'calendar.source.mail'
  return 'calendar.source.own'
}

function timeOf(ev: WireCalendarEvent, locale: string): string {
  const span = spanOf(ev)
  if (!span) return ''
  return new Date(span.start).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })
}
