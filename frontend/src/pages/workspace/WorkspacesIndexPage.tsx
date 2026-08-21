import { Folder, Plus } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { OverlayLink } from '@/components/layout/overlayRouting'
import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'

export function WorkspacesIndexPage() {
  const { t } = useTranslation('workspaces')
  return (
    <PageState
      icon={Folder}
      title={t('list.selectTitle')}
      description={t('list.selectDescription')}
      action={
        <Button size="sm" variant="default" asChild>
          <OverlayLink to="/workspaces/new">
            <Plus className="h-3.5 w-3.5" />
            {t('new')}
          </OverlayLink>
        </Button>
      }
    />
  )
}
