import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => {
  const changeLanguage = vi.fn()
  return {
    changeLanguage,
    i18n: {
      language: 'en-US',
      resolvedLanguage: 'en-US',
      changeLanguage,
    },
    useSystemSettings: vi.fn(),
    writeLanguageMirror: vi.fn(),
  }
})

vi.mock('@/hooks/useSystemSettings', () => ({
  useSystemSettings: mocks.useSystemSettings,
}))

vi.mock('@/i18n', () => ({
  default: mocks.i18n,
  writeLanguageMirror: mocks.writeLanguageMirror,
}))

import { useApplyLanguage } from '@/hooks/useApplyLanguage'

describe('useApplyLanguage', () => {
  afterEach(() => {
    document.documentElement.lang = ''
    mocks.i18n.language = 'en-US'
    mocks.i18n.resolvedLanguage = 'en-US'
    vi.clearAllMocks()
  })

  it('applies and mirrors the server-confirmed language', async () => {
    mocks.useSystemSettings.mockReturnValue({ data: { language: 'zh-CN' } })

    renderHook(() => useApplyLanguage())

    await waitFor(() => {
      expect(mocks.changeLanguage).toHaveBeenCalledWith('zh-CN')
      expect(document.documentElement.lang).toBe('zh-CN')
      expect(mocks.writeLanguageMirror).toHaveBeenCalledWith('zh-CN')
    })
  })

  it('does not persist or apply a language before settings are confirmed', () => {
    mocks.useSystemSettings.mockReturnValue({ data: undefined })

    renderHook(() => useApplyLanguage())

    expect(mocks.changeLanguage).not.toHaveBeenCalled()
    expect(document.documentElement.lang).toBe('')
    expect(mocks.writeLanguageMirror).not.toHaveBeenCalled()
  })

  it('does not ask i18next to change when it already uses the server language', async () => {
    mocks.i18n.language = 'zh-CN'
    mocks.i18n.resolvedLanguage = 'zh-CN'
    mocks.useSystemSettings.mockReturnValue({ data: { language: 'zh-CN' } })

    renderHook(() => useApplyLanguage())

    await waitFor(() => {
      expect(mocks.changeLanguage).not.toHaveBeenCalled()
      expect(document.documentElement.lang).toBe('zh-CN')
      expect(mocks.writeLanguageMirror).toHaveBeenCalledWith('zh-CN')
    })
  })
})
