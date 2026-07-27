import { HardDrive } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { PageState } from '@/components/ui/page-state'

export function WorkspacesIndexPage() {
  const { t } = useTranslation('workspaces')
  return (
    <PageState
      icon={HardDrive}
      title={t('list.selectTitle')}
      description={t('list.selectDescription')}
    />
  )
}
