import { describe, expect, it } from 'vitest'

import { replySegments } from '@/lib/replySegments'

describe('replySegments', () => {
  it('uses the recorded segments when they spell out the content', () => {
    expect(replySegments('ab', ['a', 'b'])).toEqual(['a', 'b'])
  })

  it('puts text appended by a resume in a bubble of its own', () => {
    expect(replySegments('Hello continued', ['Hello'])).toEqual(['Hello', ' continued'])
  })

  it('keeps text the segments never recorded, such as a provider error, apart', () => {
    expect(replySegments('Usage limit. Done.', [' Done.'])).toEqual(['Usage limit.', ' Done.'])
  })

  it('falls back to one bubble when the segments cannot be placed', () => {
    expect(replySegments('abc', ['x'])).toEqual(['abc'])
    expect(replySegments('abc', null)).toEqual(['abc'])
  })
})
