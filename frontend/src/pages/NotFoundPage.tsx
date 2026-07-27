import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { Compass } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'

export function NotFoundPage() {
  const { t } = useTranslation('common')
  const title = t('pageNotFound')

  useEffect(() => {
    document.title = title
  }, [title])

  return (
    <PageState
      icon={Compass}
      title={title}
      action={
        <Button asChild variant="outline">
          <Link to="/">{t('backToApp')}</Link>
        </Button>
      }
    />
  )
}
