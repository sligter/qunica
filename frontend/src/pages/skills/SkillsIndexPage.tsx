import { Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { PageState } from '@/components/ui/page-state'

export function SkillsIndexPage() {
  const { t } = useTranslation('skills')
  return (
    <PageState
      icon={Sparkles}
      title={t('list.selectTitle')}
      description={t('list.selectDescription')}
    />
  )
}
