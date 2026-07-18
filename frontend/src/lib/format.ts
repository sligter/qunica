import type { Language } from '@/types/api'

type DateTimeValue = Date | number | string

function toDate(value: DateTimeValue): Date {
  return value instanceof Date ? value : new Date(value)
}

export function formatDateTime(value: DateTimeValue, language: Language): string {
  return new Intl.DateTimeFormat(language, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(toDate(value))
}

export function formatTime(value: DateTimeValue, language: Language): string {
  return new Intl.DateTimeFormat(language, {
    hour: '2-digit',
    minute: '2-digit',
  }).format(toDate(value))
}

export function formatNumber(value: number, language: Language): string {
  return new Intl.NumberFormat(language).format(value)
}

export function formatRelativeTime(
  value: DateTimeValue,
  language: Language,
  now = new Date(),
): string {
  const seconds = (toDate(value).getTime() - now.getTime()) / 1_000
  const absoluteSeconds = Math.abs(seconds)
  let divisor: number
  let unit: Intl.RelativeTimeFormatUnit

  if (absoluteSeconds < 60) {
    divisor = 1
    unit = 'second'
  } else if (absoluteSeconds < 60 * 60) {
    divisor = 60
    unit = 'minute'
  } else if (absoluteSeconds < 24 * 60 * 60) {
    divisor = 60 * 60
    unit = 'hour'
  } else {
    divisor = 24 * 60 * 60
    unit = 'day'
  }

  const relativeValue = Math.sign(seconds) * Math.round(absoluteSeconds / divisor)
  return new Intl.RelativeTimeFormat(language, {
    numeric: 'auto',
    style: 'narrow',
  }).format(relativeValue, unit)
}
