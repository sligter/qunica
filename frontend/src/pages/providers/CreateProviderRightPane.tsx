import { useNavigate } from 'react-router-dom'

import { CreateProviderForm } from '@/components/providers/CreateProviderForm'

export function CreateProviderRightPane() {
  const navigate = useNavigate()
  return (
    <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
      <div className="mx-auto w-full max-w-xl space-y-4 p-8">
        <header className="space-y-1">
          <h1 className="font-serif text-xl font-semibold tracking-tight">New LLM provider</h1>
          <p className="text-sm text-muted-foreground">
            Register a chat-completion endpoint. The API key is stored securely
            and shown masked on the detail page.
          </p>
        </header>
        <CreateProviderForm onCreated={(id) => void navigate(`/providers/${id}`)} />
      </div>
    </div>
  )
}
