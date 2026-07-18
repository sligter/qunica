import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { Button } from '@/components/ui/button'

export function NotFoundPage() {
  const { t } = useTranslation('common')
  const title = t('pageNotFound')

  useEffect(() => {
    document.title = title
  }, [title])

  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 p-8 text-center">
      <h1 className="font-serif text-2xl font-semibold tracking-tight">{title}</h1>
      <Button asChild variant="outline">
        <Link to="/">{t('backToApp')}</Link>
      </Button>
    </div>
  )
}
