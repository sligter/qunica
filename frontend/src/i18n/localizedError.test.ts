import { describe, expect, it } from 'vitest'

import i18n from '@/i18n'

import { localizedErrorText, messageError, translatedError } from './localizedError'

describe('localizedError', () => {
  it('keeps raw diagnostics while translating their error framing', async () => {
    const error = messageError('RAW_BACKEND_DETAIL')

    await i18n.changeLanguage('en-US')
    expect(localizedErrorText(error, i18n.t.bind(i18n))).toBe('Error: RAW_BACKEND_DETAIL')

    await i18n.changeLanguage('zh-CN')
    expect(localizedErrorText(error, i18n.t.bind(i18n))).toBe('错误：RAW_BACKEND_DETAIL')

    await i18n.changeLanguage('en-US')
  })

  it('continues to resolve semantic fallback keys', () => {
    expect(localizedErrorText(translatedError('common:errors.unexpected'), i18n.t.bind(i18n)))
      .toBe('Something went wrong. Try again.')
  })
})
