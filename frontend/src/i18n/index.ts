import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'

import type { Language } from '../types/api'
import { enUS } from './resources/en-US'
import { zhCN } from './resources/zh-CN'

export type { Language }
export const SUPPORTED_LANGUAGES: readonly Language[] = ['zh-CN', 'en-US']
export const LANGUAGE_MIRROR_KEY = 'qunica:language'

export function normalizeLanguage(value: unknown): Language | null {
  return value === 'zh-CN' || value === 'en-US' ? value : null
}

export function readLanguageMirror(): Language | null {
  try {
    return normalizeLanguage(localStorage.getItem(LANGUAGE_MIRROR_KEY))
  } catch {
    return null
  }
}

export function writeLanguageMirror(language: Language): void {
  try {
    localStorage.setItem(LANGUAGE_MIRROR_KEY, language)
  } catch {
    // Persistence failure must not block rendering.
  }
}

export function detectBootstrapLanguage(): Language {
  const mirrored = readLanguageMirror()
  if (mirrored) return mirrored
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US'
}

void i18n.use(initReactI18next).init({
  lng: detectBootstrapLanguage(),
  fallbackLng: 'en-US',
  supportedLngs: SUPPORTED_LANGUAGES,
  defaultNS: 'common',
  ns: Object.keys(enUS),
  resources: {
    'en-US': enUS,
    'zh-CN': zhCN,
  },
  interpolation: { escapeValue: false },
  returnNull: false,
  parseMissingKeyHandler: () => '',
  saveMissing: import.meta.env.DEV,
  missingKeyHandler: import.meta.env.DEV
    ? (_languages, namespace, key) => console.warn(`Missing i18n key: ${namespace}:${key}`)
    : undefined,
})

export default i18n
