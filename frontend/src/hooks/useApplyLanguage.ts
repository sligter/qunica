import { useEffect } from 'react'

import { useSystemSettings } from '@/hooks/useSystemSettings'
import i18n, { writeLanguageMirror } from '@/i18n'

export function useApplyLanguage(): void {
  const settings = useSystemSettings()
  const serverLanguage = settings.data?.language

  useEffect(() => {
    if (!serverLanguage) return

    if ((i18n.resolvedLanguage ?? i18n.language) !== serverLanguage) {
      void i18n.changeLanguage(serverLanguage)
    }
    document.documentElement.lang = serverLanguage
    writeLanguageMirror(serverLanguage)
  }, [serverLanguage])
}
