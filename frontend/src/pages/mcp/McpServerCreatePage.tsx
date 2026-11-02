import { useLocation, useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { DetailShell } from '@/components/layout/DetailShell'
import { McpServerForm } from '@/components/mcp/McpServerForm'
import { parseMcpPrefill } from '@/components/assistant/assistantPrefill'

export function McpServerCreatePage() {
  const navigate = useNavigate()
  const { search } = useLocation()
  const { t } = useTranslation('mcp')
  return (
    <DetailShell title={t('form.createTitle')} subtitle={t('form.createSubtitle')}>
      <McpServerForm key={search} prefill={parseMcpPrefill(search)} onSaved={(server) => void navigate(`/mcp-servers/${server.id}`)} />
    </DetailShell>
  )
}
