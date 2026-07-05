import { useNavigate } from 'react-router-dom'

import { CreateProviderForm } from '@/components/providers/CreateProviderForm'
import { DetailShell } from '@/components/layout/DetailShell'

export function ProviderCreatePage() {
  const navigate = useNavigate()
  return (
    <DetailShell
      title="New LLM provider"
      subtitle="Register a chat-completion endpoint. The API key is stored securely and shown masked on the detail page."
    >
      <CreateProviderForm onCreated={(id) => void navigate(`/providers/${id}`)} />
    </DetailShell>
  )
}
