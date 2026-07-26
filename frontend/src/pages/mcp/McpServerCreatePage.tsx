import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { DetailShell } from '@/components/layout/DetailShell'
import { McpServerForm } from '@/components/mcp/McpServerForm'

export function McpServerCreatePage() {
  const navigate = useNavigate()
  const { t } = useTranslation('mcp')
  return (
    <DetailShell title={t('form.createTitle')} subtitle={t('form.createSubtitle')}>
      <McpServerForm onSaved={(server) => void navigate(`/mcp-servers/${server.id}`)} />
    </DetailShell>
  )
}
