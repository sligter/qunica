import { Server } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { PageState } from '@/components/ui/page-state'

export function McpServersIndexPage() {
  const { t } = useTranslation('mcp')
  return (
    <PageState
      icon={Server}
      title={t('list.selectTitle')}
      description={t('list.selectDescription')}
    />
  )
}
