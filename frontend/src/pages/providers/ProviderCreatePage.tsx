import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { CreateProviderForm } from '@/components/providers/CreateProviderForm'
import { DetailShell } from '@/components/layout/DetailShell'

export function ProviderCreatePage() {
  const navigate = useNavigate()
  const { t } = useTranslation('providers')
  return (
    <DetailShell
      title={t('form.createTitle')}
      subtitle={t('form.createSubtitle')}
    >
      <CreateProviderForm onCreated={(provider) => void navigate(`/providers/${provider.id}`)} />
    </DetailShell>
  )
}
