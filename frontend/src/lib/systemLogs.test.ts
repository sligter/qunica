import { describe, expect, it } from 'vitest'

import { formatLogFilter, parseLogFilter } from '@/lib/systemLogs'

describe('system log filters', () => {
  it('round-trips the collection level and module overrides', () => {
    const parsed = parseLogFilter('info,qunica_backend::api=debug')

    expect(parsed).toEqual({
      level: 'info',
      overrides: [
        { target: 'qunica_backend::api', level: 'debug' },
      ],
    })
    expect(formatLogFilter(parseLogFilter('warn,app=trace'))).toBe(
      'warn,app=trace',
    )
  })
})
