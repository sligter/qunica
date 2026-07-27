import { Plug } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { PageState } from '@/components/ui/page-state'

export function ProvidersIndexPage() {
  const { t } = useTranslation('providers')
  return (
    <PageState
      icon={Plug}
      title={t('list.selectTitle')}
      description={t('list.selectDescription')}
    />
  )
}
