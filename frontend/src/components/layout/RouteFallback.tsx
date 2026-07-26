import { useTranslation } from 'react-i18next'

/**
 * Quiet placeholder shown while a lazily-loaded route chunk downloads. Centered
 * and text-only so it never shifts the shell around it.
 */
export function RouteFallback() {
  const { t } = useTranslation('common')

  return (
    <div
      className="flex h-full w-full items-center justify-center p-6"
      role="status"
      aria-live="polite"
    >
      <span className="text-sm text-muted-foreground">{t('state.loading')}</span>
    </div>
  )
}
