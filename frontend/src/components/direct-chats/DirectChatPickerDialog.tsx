import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useAgents } from '@/hooks/useAgents'
import { useCreateDirectChat } from '@/hooks/useDirectChats'

interface DirectChatPickerDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

interface DirectChatPickerBodyProps {
  onOpenChange: (open: boolean) => void
}

/**
 * Picker body. Lives inside `DialogContent` so the agent list is fetched when the
 * dialog opens rather than when the sidebar that owns it first renders.
 */
function DirectChatPickerBody({ onOpenChange }: DirectChatPickerBodyProps) {
  const { t } = useTranslation(['chat', 'common'])
  const navigate = useNavigate()
  const agents = useAgents()
  const createChat = useCreateDirectChat()
  const [query, setQuery] = useState('')
  const [error, setError] = useState<string | null>(null)
  const activeAgents = useMemo(() => {
    const normalized = query.trim().toLowerCase()
    return (agents.data ?? []).filter(
      (agent) =>
        agent.status === 'active' &&
        (!normalized ||
          agent.name.toLowerCase().includes(normalized) ||
          (agent.description ?? '').toLowerCase().includes(normalized)),
    )
  }, [agents.data, query])

  const selectAgent = async (agentId: string) => {
    setError(null)
    try {
      const chat = await createChat.mutateAsync({ agent_id: agentId })
      onOpenChange(false)
      void navigate(`/chats/${chat.id}`)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    }
  }

  return (
    <>
      <DialogHeader>
        <DialogTitle>{t('direct.pickerTitle')}</DialogTitle>
        <DialogDescription>{t('direct.pickerDescription')}</DialogDescription>
      </DialogHeader>
      <Input
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        placeholder={t('direct.searchAgents')}
        aria-label={t('direct.searchAgents')}
      />
      <div className="max-h-80 space-y-1 overflow-y-auto">
        {agents.isLoading ? (
          <p className="p-2 text-sm text-muted-foreground">{t('common:state.loading')}</p>
        ) : null}
        {agents.error ? <p className="p-2 text-sm text-destructive">{String(agents.error)}</p> : null}
        {!agents.isLoading && !agents.error && activeAgents.length === 0 ? (
          <p className="p-2 text-sm text-muted-foreground">{t('direct.noAgents')}</p>
        ) : null}
        {activeAgents.map((agent) => (
          <Button
            key={agent.id}
            type="button"
            variant="ghost"
            className="h-auto w-full justify-start px-3 py-2 text-left"
            disabled={createChat.isPending}
            onClick={() => void selectAgent(agent.id)}
          >
            <span className="min-w-0">
              <span className="block truncate text-sm font-medium">{agent.name}</span>
              {agent.description ? (
                <span className="block truncate text-xs text-muted-foreground">
                  {agent.description}
                </span>
              ) : null}
            </span>
          </Button>
        ))}
      </div>
      {createChat.isPending ? (
        <p className="text-xs text-muted-foreground">{t('direct.creating')}</p>
      ) : null}
      {error ? <p role="alert" className="text-sm text-destructive">{error}</p> : null}
    </>
  )
}

export function DirectChatPickerDialog({ open, onOpenChange }: DirectChatPickerDialogProps) {
  const { t } = useTranslation('common')

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent closeLabel={t('actions.close')} className="sm:max-w-lg">
        <DirectChatPickerBody onOpenChange={onOpenChange} />
      </DialogContent>
    </Dialog>
  )
}
