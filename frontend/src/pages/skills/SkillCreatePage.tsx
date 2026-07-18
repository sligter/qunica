import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { ImportSkillForm } from '@/components/skills/ImportSkillForm'
import { DetailShell } from '@/components/layout/DetailShell'

export function SkillCreatePage() {
  const navigate = useNavigate()
  const { t } = useTranslation('skills')
  return (
    <DetailShell
      title={t('form.createTitle')}
      subtitle={t('form.createSubtitle')}
    >
      <ImportSkillForm onCreated={(id) => void navigate(`/skills/${id}`)} />
    </DetailShell>
  )
}
