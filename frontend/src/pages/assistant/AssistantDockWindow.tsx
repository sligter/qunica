/**
 * Standalone host page for the Assistant dock window.
 *
 * The native always-on-top Assistant window loads this route instead of the
 * full app shell: no sidebar, no terminal dock, no context menu. Compact
 * `ConversationChatView` still talks to the same chat APIs, but it must not
 * assume a `TerminalRuntimeProvider` — this window never mounts one.
 */

import { useCallback, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { GripHorizontal, Minus, Settings2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AssistantSettings } from '@/components/assistant/AssistantSettings'
import { AssistantSetupChecklist } from '@/components/assistant/AssistantSetupChecklist'
import { ConversationChatView } from '@/components/chat/ConversationChatView'
import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'
import { useAssistant } from '@/hooks/useAssistant'
import { useProvider } from '@/hooks/useProviders'
import {
  hideCurrentDesktopWindow,
  startDraggingCurrentDesktopWindow,
} from '@/lib/desktop'

export function AssistantDockWindow() {
  const { t } = useTranslation('assistant')
  const assistant = useAssistant()
  const provider = useProvider(assistant.data?.provider_id ?? undefined)
  const [showSettings, setShowSettings] = useState(false)
  const activeModel = assistant.data?.model ?? provider.data?.default_model
  const supportsReasoningEffort = Boolean(
    provider.data?.models?.find((model) => model.id === activeModel)?.supports_reasoning_effort,
  )
  const startWindowDrag = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return
    event.preventDefault()
    void startDraggingCurrentDesktopWindow().catch(() => undefined)
  }, [])

  return (
    <div className="flex h-screen min-h-0 flex-col overflow-hidden bg-background">
      <div
        data-testid="assistant-window-drag-handle"
        onPointerDown={startWindowDrag}
        className="flex h-11 shrink-0 cursor-grab select-none items-center justify-between border-b border-border/60 px-3 active:cursor-grabbing"
      >
        <div className="flex min-w-0 items-center gap-2">
          <GripHorizontal
            className="h-4 w-4 shrink-0 text-muted-foreground/70"
            aria-hidden
          />
          <h1
            className="truncate font-serif text-sm font-semibold tracking-tight"
          >
            {t('chat.title')}
          </h1>
        </div>
        <div
          className="flex cursor-default items-center gap-1"
          onPointerDown={(event) => event.stopPropagation()}
        >
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-8 w-8 rounded-full"
            aria-label={t('settings.title')}
            aria-pressed={showSettings}
            onClick={() => setShowSettings((open) => !open)}
          >
            <Settings2 className="h-4 w-4" aria-hidden />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-8 w-8 rounded-full"
            aria-label={t('collapse')}
            onClick={() => void hideCurrentDesktopWindow().catch(() => undefined)}
          >
            <Minus className="h-4 w-4" aria-hidden />
          </Button>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-hidden">
        {assistant.isLoading ? (
          <PageState variant="loading" title={t('status.loading')} />
        ) : showSettings ? (
          <AssistantSettings onClose={() => setShowSettings(false)} />
        ) : assistant.data?.provider_configured && assistant.data.chat_id ? (
          <ConversationChatView
            conversationId={assistant.data.chat_id}
            workspaceId={null}
            supportsReasoningEffort={supportsReasoningEffort}
            scope="direct-chats"
            agents={[]}
            agentIsSystem
            compact
            title={t('chat.title')}
            capabilities={{
              showAnnouncement: false,
              showManage: false,
              showTurnTrace: false,
              showWorkspace: false,
              showTerminal: false,
              allowMentions: false,
            }}
          />
        ) : (
          <AssistantSetupChecklist
            loading={assistant.isLoading}
            error={Boolean(assistant.error)}
          />
        )}
      </div>
    </div>
  )
}
