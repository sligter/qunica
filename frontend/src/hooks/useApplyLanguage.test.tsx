import { cleanup, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useAuthStore } from '@/stores/authStore'

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
  beforeEach(() => {
    useAuthStore.setState({
      user: {
        id: 'current-owner',
        email: 'owner@example.com',
        name: 'Owner',
        avatar_url: null,
        created_at: '2026-07-18T00:00:00Z',
      },
    })
  })

  afterEach(() => {
    cleanup()
    document.documentElement.lang = ''
    mocks.i18n.language = 'en-US'
    mocks.i18n.resolvedLanguage = 'en-US'
    useAuthStore.setState({ user: null })
    vi.clearAllMocks()
  })

  it('applies and mirrors the server-confirmed language', async () => {
    mocks.useSystemSettings.mockReturnValue({
      data: { owner_id: 'current-owner', language: 'zh-CN' },
    })

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
    mocks.useSystemSettings.mockReturnValue({
      data: { owner_id: 'current-owner', language: 'zh-CN' },
    })

    renderHook(() => useApplyLanguage())

    await waitFor(() => {
      expect(mocks.changeLanguage).not.toHaveBeenCalled()
      expect(document.documentElement.lang).toBe('zh-CN')
      expect(mocks.writeLanguageMirror).toHaveBeenCalledWith('zh-CN')
    })
  })

  it('ignores cached settings that belong to another account', () => {
    document.documentElement.lang = 'en-US'
    mocks.useSystemSettings.mockReturnValue({
      data: { owner_id: 'previous-owner', language: 'zh-CN' },
    })

    renderHook(() => useApplyLanguage())

    expect(mocks.changeLanguage).not.toHaveBeenCalled()
    expect(document.documentElement.lang).toBe('en-US')
    expect(mocks.writeLanguageMirror).not.toHaveBeenCalled()
  })
})
