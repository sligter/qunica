import { expect, it } from 'vitest'
import { controlInput } from './controlInput'

it('encodes ASCII chords and preserves IME input and escape sequences', () => {
  expect(controlInput('c')).toBe('\x03')
  expect(controlInput('d')).toBe('\x04')
  expect(controlInput('[')).toBe('\x1b')
  expect(controlInput('?')).toBe('\x7f')
  expect(controlInput('中文')).toBe('中文')
  expect(controlInput('\x1b[A')).toBe('\x1b[A')
})
