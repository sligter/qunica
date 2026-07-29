import { describe, expect, it } from 'vitest'

import { formatLogFilter, parseLogFilter } from '@/lib/systemLogs'

describe('system log filters', () => {
  it('round-trips the collection level and module overrides', () => {
    const parsed = parseLogFilter('info,ag_swarmer_backend::api=debug')

    expect(parsed).toEqual({
      level: 'info',
      overrides: [
        { target: 'ag_swarmer_backend::api', level: 'debug' },
      ],
    })
    expect(formatLogFilter(parseLogFilter('warn,app=trace'))).toBe(
      'warn,app=trace',
    )
  })
})
