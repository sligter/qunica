import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { CreateAgentForm } from '@/components/agents/CreateAgentForm'
import { DetailShell } from '@/components/layout/DetailShell'

export function AgentCreatePage() {
  const navigate = useNavigate()
  const { t } = useTranslation('agents')
  return (
    <DetailShell
      title={t('form.createTitle')}
      subtitle={t('form.createSubtitle')}
    >
      <CreateAgentForm onCreated={(id) => void navigate(`/agents/${id}`)} />
    </DetailShell>
  )
}
