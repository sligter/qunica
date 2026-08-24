import { cleanup, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useAuthStore } from '@/stores/authStore'

const mocks = vi.hoisted(() => ({
  useSystemSettings: vi.fn(),
}))

vi.mock('@/hooks/useSystemSettings', () => ({
  useSystemSettings: mocks.useSystemSettings,
}))

import {
  APPEARANCE_MIRROR_KEY,
  useApplyAppearance,
} from '@/hooks/useApplyAppearance'

const currentUser = {
  id: 'current-owner',
  email: 'owner@example.com',
  name: 'Owner',
  avatar_url: null,
  created_at: '2026-08-24T00:00:00Z',
}

function mockSystemAppearance(dark: boolean): void {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    writable: true,
    value: vi.fn().mockImplementation(() => ({
      matches: dark,
      media: '(prefers-color-scheme: dark)',
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  })
}

describe('useApplyAppearance', () => {
  beforeEach(() => {
    localStorage.clear()
    mockSystemAppearance(false)
    useAuthStore.setState({
      token: 'token',
      user: currentUser,
      hydrated: true,
    })
  })

  afterEach(() => {
    cleanup()
    delete document.documentElement.dataset.theme
    document.documentElement.style.colorScheme = ''
    localStorage.clear()
    useAuthStore.setState({ token: null, user: null, hydrated: false })
    vi.clearAllMocks()
  })

  it('applies and mirrors the server-confirmed appearance', async () => {
    mocks.useSystemSettings.mockReturnValue({
      data: { owner_id: 'current-owner', appearance: 'dark' },
    })

    renderHook(() => useApplyAppearance())

    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe('dark')
      expect(document.documentElement.style.colorScheme).toBe('dark')
      expect(localStorage.getItem(APPEARANCE_MIRROR_KEY)).toBe('dark')
    })
  })

  it('keeps the bootstrap theme while authenticated settings are loading', () => {
    document.documentElement.dataset.theme = 'dark'
    document.documentElement.style.colorScheme = 'dark'
    localStorage.setItem(APPEARANCE_MIRROR_KEY, 'dark')
    mocks.useSystemSettings.mockReturnValue({ data: undefined })

    renderHook(() => useApplyAppearance())

    expect(document.documentElement.dataset.theme).toBe('dark')
    expect(document.documentElement.style.colorScheme).toBe('dark')
    expect(localStorage.getItem(APPEARANCE_MIRROR_KEY)).toBe('dark')
  })

  it('ignores cached settings that belong to another account', () => {
    document.documentElement.dataset.theme = 'light'
    localStorage.setItem(APPEARANCE_MIRROR_KEY, 'light')
    mocks.useSystemSettings.mockReturnValue({
      data: { owner_id: 'previous-owner', appearance: 'dark' },
    })

    renderHook(() => useApplyAppearance())

    expect(document.documentElement.dataset.theme).toBe('light')
    expect(localStorage.getItem(APPEARANCE_MIRROR_KEY)).toBe('light')
  })

  it('uses and mirrors the system theme while signed out', async () => {
    mockSystemAppearance(true)
    useAuthStore.setState({ token: null, user: null, hydrated: true })
    mocks.useSystemSettings.mockReturnValue({ data: undefined })

    renderHook(() => useApplyAppearance())

    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe('dark')
      expect(localStorage.getItem(APPEARANCE_MIRROR_KEY)).toBe('dark')
    })
  })

  it('synchronizes appearance changes from another native window', () => {
    mocks.useSystemSettings.mockReturnValue({ data: undefined })
    renderHook(() => useApplyAppearance())

    window.dispatchEvent(new StorageEvent('storage', {
      key: APPEARANCE_MIRROR_KEY,
      newValue: 'dark',
    }))

    expect(document.documentElement.dataset.theme).toBe('dark')
    expect(document.documentElement.style.colorScheme).toBe('dark')
  })
})
