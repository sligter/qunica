/** Display and form helpers shared by the automation list and editor. */

export function formatAutomationTime(
  value: number | string | null | undefined,
  locale: string,
  timeZone?: string,
): string {
  if (value === null || value === undefined || value === '') return '—'
  const date = typeof value === 'number' ? new Date(value * 1000) : new Date(value)
  if (Number.isNaN(date.getTime())) return '—'
  return new Intl.DateTimeFormat(locale, {
    dateStyle: 'medium',
    timeStyle: 'short',
    timeZone,
  }).format(date)
}

/** A compact "in 2 hours" label for the next run, or null when unset. */
export function formatRelativeRun(
  epochSeconds: number | null,
  locale: string,
  now = Date.now(),
): string | null {
  if (epochSeconds === null) return null
  const deltaSeconds = epochSeconds - Math.floor(now / 1000)
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' })
  const absolute = Math.abs(deltaSeconds)
  if (absolute < 60) return formatter.format(Math.round(deltaSeconds), 'second')
  if (absolute < 3600) return formatter.format(Math.round(deltaSeconds / 60), 'minute')
  if (absolute < 86_400) return formatter.format(Math.round(deltaSeconds / 3600), 'hour')
  return formatter.format(Math.round(deltaSeconds / 86_400), 'day')
}

function pad(value: number): string {
  return String(value).padStart(2, '0')
}

/** RFC3339 instant → the local `datetime-local` input value. */
export function toLocalDateTimeInput(value: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return ''
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}`
}

/** Local `datetime-local` input value → an RFC3339 UTC instant. */
export function fromLocalDateTimeInput(value: string): string | null {
  if (!value) return null
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? null : date.toISOString()
}

export function defaultOnceAt(): string {
  const date = new Date()
  date.setDate(date.getDate() + 1)
  date.setHours(9, 0, 0, 0)
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}`
}

export function browserTimezone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'
  } catch {
    return 'UTC'
  }
}
