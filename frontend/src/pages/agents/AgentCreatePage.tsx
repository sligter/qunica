import { useNavigate } from 'react-router-dom'

import { CreateAgentForm } from '@/components/agents/CreateAgentForm'
import { DetailShell } from '@/components/layout/DetailShell'

export function AgentCreatePage() {
  const navigate = useNavigate()
  return (
    <DetailShell
      title="New agent"
      subtitle="Define an agent's name, system prompt, and optional model parameters."
    >
      <CreateAgentForm onCreated={(id) => void navigate(`/agents/${id}`)} />
    </DetailShell>
  )
}
