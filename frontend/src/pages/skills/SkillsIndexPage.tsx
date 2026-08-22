import { Plus, Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { OverlayLink } from '@/components/layout/overlayRouting'
import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'

export function SkillsIndexPage() {
  const { t } = useTranslation('skills')
  return (
    <PageState
      icon={Sparkles}
      title={t('list.selectTitle')}
      description={t('list.selectDescription')}
      action={
        <Button size="sm" variant="default" asChild>
          <OverlayLink to="/skills/new">
            <Plus className="h-3.5 w-3.5" />
            {t('import')}
          </OverlayLink>
        </Button>
      }
    />
  )
}
