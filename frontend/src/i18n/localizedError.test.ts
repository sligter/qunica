import { describe, expect, it } from 'vitest'

import i18n from '@/i18n'
import { ApiError } from '@/lib/api-v2/client'

import {
  localizedErrorText,
  messageError,
  translatedError,
  workspaceErrorMessageKey,
} from './localizedError'

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

  it('maps workspace API diagnostics to semantic localized messages', async () => {
    const rawMessage = 'workspace file is not valid UTF-8 text'
    const error = new ApiError(400, 'invalid_input', rawMessage)
    const key = workspaceErrorMessageKey(error)

    await i18n.changeLanguage('en-US')
    expect(i18n.t(`chat:${key}`)).toBe(
      'The workspace request was rejected. Check the file or folder and try again.',
    )

    await i18n.changeLanguage('zh-CN')
    const chinese = i18n.t(`chat:${key}`)
    expect(chinese).toBe('请求未通过，请检查文件或文件夹后重试。')
    expect(chinese).not.toContain(rawMessage)

    await i18n.changeLanguage('en-US')
  })

  it('does not expose unknown internal workspace errors', async () => {
    const rawMessage = 'Workspace upload returned no file'
    const key = workspaceErrorMessageKey(new Error(rawMessage))

    await i18n.changeLanguage('zh-CN')
    expect(i18n.t(`chat:${key}`)).toBe('无法完成此工作区操作。')
    expect(i18n.t(`chat:${key}`)).not.toContain(rawMessage)

    await i18n.changeLanguage('en-US')
  })
})
