import { Bot } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { PageState } from '@/components/ui/page-state'

export function AgentsIndexPage() {
  const { t } = useTranslation('agents')
  return (
    <PageState
      icon={Bot}
      title={t('list.selectTitle')}
      description={t('list.selectDescription')}
    />
  )
}
