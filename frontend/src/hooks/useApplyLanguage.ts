import { useEffect } from 'react'

import { useSystemSettings } from '@/hooks/useSystemSettings'
import i18n, { writeLanguageMirror } from '@/i18n'
import { useAuthStore } from '@/stores/authStore'

export function useApplyLanguage(): void {
  const currentUserId = useAuthStore((state) => state.user?.id)
  const settings = useSystemSettings()
  const serverLanguage =
    currentUserId !== undefined &&
    settings.data?.owner_id === currentUserId &&
    settings.data.onboarding_completed !== false
      ? settings.data.language
      : undefined

  useEffect(() => {
    if (!serverLanguage) return

    if ((i18n.resolvedLanguage ?? i18n.language) !== serverLanguage) {
      void i18n.changeLanguage(serverLanguage)
    }
    document.documentElement.lang = serverLanguage
    writeLanguageMirror(serverLanguage)
  }, [serverLanguage])
}
