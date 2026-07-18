import { afterEach, describe, expect, it, vi } from 'vitest'

import { enUS } from './resources/en-US'
import { zhCN } from './resources/zh-CN'
import {
  LANGUAGE_MIRROR_KEY,
  detectBootstrapLanguage,
  normalizeLanguage,
} from './index'

function keys(value: unknown, prefix = ''): string[] {
  if (typeof value !== 'object' || value === null) return [prefix]
  return Object.entries(value).flatMap(([key, child]) =>
    keys(child, prefix ? `${prefix}.${key}` : key),
  )
}

describe('i18n bootstrap', () => {
  afterEach(() => {
    localStorage.clear()
    vi.unstubAllGlobals()
  })

  it('keeps the two supported locale values only', () => {
    expect(normalizeLanguage('zh-CN')).toBe('zh-CN')
    expect(normalizeLanguage('en-US')).toBe('en-US')
    expect(normalizeLanguage('fr-FR')).toBeNull()
  })

  it('prefers the local mirror before browser detection', () => {
    localStorage.setItem(LANGUAGE_MIRROR_KEY, 'en-US')
    vi.stubGlobal('navigator', { language: 'zh-CN' })
    expect(detectBootstrapLanguage()).toBe('en-US')
  })

  it('maps Chinese browser locales to zh-CN and everything else to en-US', () => {
    vi.stubGlobal('navigator', { language: 'zh-TW' })
    expect(detectBootstrapLanguage()).toBe('zh-CN')
    vi.stubGlobal('navigator', { language: 'de-DE' })
    expect(detectBootstrapLanguage()).toBe('en-US')
  })

  it('keeps English and Chinese resource keys identical', () => {
    expect(keys(zhCN).sort()).toEqual(keys(enUS).sort())
  })
})
