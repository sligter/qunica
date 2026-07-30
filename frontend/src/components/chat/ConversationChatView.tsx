import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Files, PanelRightClose, SquareTerminal } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Composer } from '@/components/chat/Composer'
import { DirectChatHeaderActions } from '@/components/direct-chats/DirectChatHeaderActions'
import { GroupWorkspacePanel } from '@/components/chat/GroupWorkspacePanel'
import { MessageList } from '@/components/chat/MessageList'
import { TurnTraceDrawer } from '@/components/chat/TurnTraceDrawer'
import { VerticalResizeHandle } from '@/components/layout/VerticalResizeHandle'
import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'
import {
  type ConversationScope,
  useConversationMessages,
} from '@/hooks/useGroupMessages'
import { usePersistentPaneWidth } from '@/hooks/usePersistentPaneWidth'
import { useSendMessageStream } from '@/hooks/useSendMessageStream'
import type { ConversationUpdatedPayload } from '@/lib/api-v2/types'
import { useFileNavStore } from '@/stores/fileNavStore'
import { useMessageStore } from '@/stores/messageStore'
import { useTerminalRuntime } from '@/terminal/TerminalRuntimeProvider'
import { useTerminalConversationRegistration } from '@/terminal/useTerminalConversationRegistration'
import type { GroupAgentRead } from '@/types/api'

const WORKSPACE_FILES_OPEN_KEY_PREFIX = 'ag-swarmer:conversations:workspace-files-open:'

type WorkspaceFilesOpenUpdater = boolean | ((current: boolean) => boolean)

export interface ConversationChatViewProps {
  conversationId: string
  workspaceId: string | null
  scope: ConversationScope
  schedulerEnabled: boolean
  agents: GroupAgentRead[]
  title: React.ReactNode
  subtitle?: React.ReactNode
  announcement?: string | null
  headerActions?: React.ReactNode
  capabilities: {
    showAnnouncement: boolean
    showManage: boolean
    showTurnTrace: boolean
    showWorkspace: boolean
    allowMentions: boolean
  }
  onConversationUpdated?: (payload: ConversationUpdatedPayload) => void
  disabledComposerReason?: string
}

function workspaceFilesOpenStorageKey(conversationId: string): string {
  return `${WORKSPACE_FILES_OPEN_KEY_PREFIX}${conversationId}`
}

function readWorkspaceFilesOpen(conversationId: string): boolean {
  const value = sessionStorage.getItem(workspaceFilesOpenStorageKey(conversationId))
  return value === null ? true : value === 'true'
}

function storeWorkspaceFilesOpen(conversationId: string, value: boolean): void {
  sessionStorage.setItem(workspaceFilesOpenStorageKey(conversationId), String(value))
}

function isEditableShortcutTarget(event: globalThis.KeyboardEvent): boolean {
  const isEditableElement = (target: EventTarget): boolean => {
    if (!(target instanceof HTMLElement)) return false

    return (
      target instanceof HTMLInputElement ||
      target instanceof HTMLTextAreaElement ||
      target instanceof HTMLSelectElement ||
      target.isContentEditable ||
      target.closest('[contenteditable=true]') !== null
    )
  }

  const eventPath = event.composedPath?.() ?? []
  if (eventPath.some(isEditableElement)) return true

  const { target } = event
  const isWindowLikeTarget =
    target !== null &&
    typeof target === 'object' &&
    'window' in target &&
    (target as { window?: unknown }).window === target
  if (target === null || target === window || target === document || isWindowLikeTarget) return false

  // Do not claim a shortcut from a custom or inaccessible event target.
  return !(target instanceof HTMLElement)
}

export function ConversationChatView({
  conversationId,
  workspaceId,
  scope,
  schedulerEnabled,
  agents,
  title,
  subtitle,
  announcement,
  headerActions,
  capabilities,
  onConversationUpdated,
  disabledComposerReason,
}: ConversationChatViewProps) {
  const { t } = useTranslation('chat')
  const { isDockOpen, toggleDock } = useTerminalRuntime()
  useTerminalConversationRegistration(conversationId, workspaceId)
  const messagesQuery = useConversationMessages(scope, conversationId)
  const stream = useSendMessageStream(conversationId, schedulerEnabled, {
    scope,
    onConversationUpdated,
  })
  const sendMessage = stream.send
  const submitHumanInput = useCallback((content: string) => {
    void sendMessage(content).catch(() => undefined)
  }, [sendMessage])
  const clearWarnings = useMessageStore((state) => state.clearWarnings)
  const activeResumesByMessageId = useMessageStore(
    (state) => state.activeResumesByMessageId,
  )
  const activeResumes = useMemo(
    () => Object.values(activeResumesByMessageId).filter(
      (resume) => resume.group_id === conversationId,
    ),
    [activeResumesByMessageId, conversationId],
  )
  const isConversationStreaming = stream.isStreaming || activeResumes.length > 0
  const cancelConversationStream = useCallback(() => {
    if (stream.isStreaming) void stream.cancel()
    for (const resume of activeResumes) void resume.cancel()
  }, [activeResumes, stream])
  const fileNavRequest = useFileNavStore((state) => state.request)
  const traceTriggerRef = useRef<HTMLButtonElement | null>(null)
  const [workspaceFilesOpen, setWorkspaceFilesOpen] = useState(() =>
    capabilities.showWorkspace ? readWorkspaceFilesOpen(conversationId) : false,
  )
  const [selectedTurnId, setSelectedTurnId] = useState<string | null>(null)
  const workspaceFilesPane = usePersistentPaneWidth({
    storageKey: 'ag-swarmer:layout:workspace-files-pane-width',
    defaultWidth: 280,
    minWidth: 240,
    maxWidth: 560,
  })

  useEffect(() => {
    setWorkspaceFilesOpen(
      capabilities.showWorkspace ? readWorkspaceFilesOpen(conversationId) : false,
    )
    setSelectedTurnId(null)
    clearWarnings(conversationId)
  }, [capabilities.showWorkspace, clearWarnings, conversationId])

  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      const isMac = /mac/i.test(navigator.platform || navigator.userAgent)
      const modifier = isMac
        ? event.metaKey && !event.ctrlKey
        : event.ctrlKey && !event.metaKey
      if (
        event.key !== '`' ||
        !modifier ||
        event.altKey ||
        event.isComposing ||
        event.repeat ||
        event.defaultPrevented
      ) {
        return
      }
      if (isEditableShortcutTarget(event)) return
      event.preventDefault()
      void toggleDock().catch(() => undefined)
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [toggleDock])

  const setWorkspaceFilesOpenPersisted = useCallback(
    (next: WorkspaceFilesOpenUpdater) => {
      setWorkspaceFilesOpen((current) => {
        const resolved = typeof next === 'function' ? next(current) : next
        storeWorkspaceFilesOpen(conversationId, resolved)
        return resolved
      })
    },
    [conversationId],
  )

  const openTurnTrace = useCallback((turnId: string, trigger: HTMLButtonElement) => {
    traceTriggerRef.current = trigger
    setSelectedTurnId(turnId)
  }, [])

  useEffect(() => {
    if (capabilities.showWorkspace && fileNavRequest?.groupId === conversationId) {
      setWorkspaceFilesOpenPersisted(true)
    }
  }, [capabilities.showWorkspace, conversationId, fileNavRequest, setWorkspaceFilesOpenPersisted])

  if (messagesQuery.error) {
    return <PageState variant="error" title={String(messagesQuery.error)} />
  }

  if (messagesQuery.isLoading) {
    return <PageState variant="loading" title={t('messages.loading')} />
  }

  const hint = agents.length === 0 ? t('composer.noAgents') : undefined

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-14 shrink-0 items-center justify-between gap-4 border-b border-border/60 bg-background px-4 lg:px-5">
        <div className="flex min-w-0 items-baseline gap-3">
          <h1 className="font-serif truncate text-base font-semibold tracking-tight">{title}</h1>
          {subtitle ? <span className="text-xs text-muted-foreground">{subtitle}</span> : null}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {scope === 'direct-chats' ? (
            <DirectChatHeaderActions
              key={conversationId}
              chatId={conversationId}
              disabled={isConversationStreaming}
            />
          ) : null}
          <Button
            variant={isDockOpen ? 'secondary' : 'ghost'}
            size="icon"
            onClick={() => void toggleDock().catch(() => undefined)}
            aria-label={isDockOpen ? t('terminal.hide') : t('terminal.show')}
            aria-pressed={isDockOpen}
          >
            <SquareTerminal className="h-4 w-4" />
          </Button>
          {capabilities.showWorkspace ? (
            <Button
              variant={workspaceFilesOpen ? 'secondary' : 'ghost'}
              size="icon"
              onClick={() => setWorkspaceFilesOpenPersisted((open) => !open)}
              aria-label={workspaceFilesOpen ? t('workspace.hide') : t('workspace.show')}
            >
              {workspaceFilesOpen ? <PanelRightClose className="h-4 w-4" /> : <Files className="h-4 w-4" />}
            </Button>
          ) : null}
          {capabilities.showManage ? headerActions : null}
        </div>
      </header>

      {capabilities.showAnnouncement && announcement ? (
        <div className="shrink-0 border-b border-border/60 bg-card px-4 py-2 text-xs text-muted-foreground lg:px-5">
          {announcement}
        </div>
      ) : null}

      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col">
          <MessageList
            groupId={conversationId}
            hasOlderMessages={messagesQuery.hasNextPage}
            isLoadingOlderMessages={messagesQuery.isFetchingNextPage}
            onLoadOlderMessages={() => void messagesQuery.fetchNextPage()}
            onSubmitHumanInput={submitHumanInput}
            onViewTurnTrace={capabilities.showTurnTrace ? openTurnTrace : undefined}
            scope={scope}
            agents={agents}
          />

          {stream.error ? (
            <div className="shrink-0 px-4">
              <div className="mx-auto w-full max-w-6xl rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
                {t('stream.error', { message: stream.error })}
              </div>
            </div>
          ) : null}

          <Composer
            key={`${scope}:${conversationId}:${workspaceId ?? 'no-workspace'}`}
            conversationId={conversationId}
            workspaceId={workspaceId}
            scope={scope}
            isStreaming={isConversationStreaming}
            onSend={sendMessage}
            onCancel={cancelConversationStream}
            hint={hint}
            groupAgents={agents}
            allowMentions={capabilities.allowMentions}
            disabledReason={disabledComposerReason}
          />
        </div>
        {capabilities.showWorkspace && workspaceFilesOpen ? (
          <>
            <VerticalResizeHandle
              label={t('workspace.resize')}
              value={workspaceFilesPane.width}
              min={workspaceFilesPane.minWidth}
              max={workspaceFilesPane.maxWidth}
              increaseOnArrowRight={false}
              onResizeStart={(event) => workspaceFilesPane.startResize(event, -1)}
              onStep={workspaceFilesPane.resizeBy}
            />
            <GroupWorkspacePanel
              scope={scope}
              groupId={conversationId}
              workspaceId={workspaceId}
              width={workspaceFilesPane.width}
            />
          </>
        ) : null}
      </div>

      {capabilities.showTurnTrace ? (
        <TurnTraceDrawer
          groupId={conversationId}
          turnId={selectedTurnId}
          open={selectedTurnId !== null}
          returnFocusRef={traceTriggerRef}
          onOpenChange={(open) => {
            if (!open) setSelectedTurnId(null)
          }}
        />
      ) : null}
    </div>
  )
}
