import { describe, expect, it } from 'vitest'

import {
  formatDateTime,
  formatNumber,
  formatRelativeTime,
  formatTime,
} from '@/lib/format'

describe('locale formatters', () => {
  it('formats date and time with the requested language', () => {
    const value = new Date(2026, 6, 18, 20, 34)

    expect(formatDateTime(value, 'en-US')).toBe('Jul 18, 2026, 8:34 PM')
    expect(formatDateTime(value, 'zh-CN')).toBe('2026年7月18日 20:34')
    expect(formatTime(value, 'en-US')).toBe('08:34 PM')
    expect(formatTime(value, 'zh-CN')).toBe('20:34')
  })

  it('formats numbers with the requested language', () => {
    expect(formatNumber(12345, 'en-US')).toBe('12,345')
    expect(formatNumber(12345, 'zh-CN')).toBe('12,345')
  })

  it('formats relative time with locale-specific narrow units', () => {
    const now = new Date('2026-07-18T12:00:00Z')
    const value = '2026-07-18T11:59:00Z'

    expect(formatRelativeTime(value, 'en-US', now)).toBe('1m ago')
    expect(formatRelativeTime(value, 'zh-CN', now)).toBe('1分钟前')
  })

  it('selects second, minute, hour, and day buckets', () => {
    const now = new Date('2026-07-18T12:00:00Z')

    expect(formatRelativeTime('2026-07-18T11:59:30Z', 'en-US', now)).toBe('30s ago')
    expect(formatRelativeTime('2026-07-18T11:30:00Z', 'en-US', now)).toBe('30m ago')
    expect(formatRelativeTime('2026-07-18T10:00:00Z', 'en-US', now)).toBe('2h ago')
    expect(formatRelativeTime('2026-07-16T12:00:00Z', 'en-US', now)).toBe('2d ago')
  })
})
