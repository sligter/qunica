import { useLocation, useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { CreateProviderForm } from '@/components/providers/CreateProviderForm'
import { DetailShell } from '@/components/layout/DetailShell'
import { parseProviderPrefill } from '@/components/assistant/assistantPrefill'

export function ProviderCreatePage() {
  const navigate = useNavigate()
  const { search } = useLocation()
  const { t } = useTranslation('providers')
  return (
    <DetailShell
      title={t('form.createTitle')}
      subtitle={t('form.createSubtitle')}
    >
      <CreateProviderForm key={search} prefill={parseProviderPrefill(search)} onCreated={(provider) => void navigate(`/providers/${provider.id}`)} />
    </DetailShell>
  )
}
