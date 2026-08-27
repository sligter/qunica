import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Files, PanelRightClose, SquareTerminal } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Composer } from '@/components/chat/Composer'
import { DirectChatHeaderActions } from '@/components/direct-chats/DirectChatHeaderActions'
import { GroupWorkspacePanel } from '@/components/chat/GroupWorkspacePanel'
import { MessageList } from '@/components/chat/MessageList'
import { TurnTraceDrawer } from '@/components/chat/TurnTraceDrawer'
import { WorkspaceEditorStage } from '@/components/chat/WorkspaceEditorStage'
import { VerticalResizeHandle } from '@/components/layout/VerticalResizeHandle'
import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'
import {
  type ConversationScope,
  useConversationMessages,
} from '@/hooks/useGroupMessages'
import { usePersistentPaneWidth } from '@/hooks/usePersistentPaneWidth'
import { useSendMessageStream } from '@/hooks/useSendMessageStream'
import { useSystemSettings } from '@/hooks/useSystemSettings'
import { MAX_RETRY_ATTEMPTS } from '@/lib/api-v2/retry'
import { cn } from '@/lib/utils'
import type { ConversationUpdatedPayload } from '@/lib/api-v2/types'
import { useConversationActivityStore } from '@/stores/conversationActivityStore'
import { useFileNavStore } from '@/stores/fileNavStore'
import { useMessageStore } from '@/stores/messageStore'
import { useQueuedMessagesStore } from '@/stores/queuedMessagesStore'
import { useOptionalTerminalRuntime } from '@/terminal/TerminalRuntimeProvider'
import { useOptionalTerminalConversationRegistration } from '@/terminal/useTerminalConversationRegistration'
import type { GroupAgentRead, MessageSendInput } from '@/types/api'

const WORKSPACE_FILES_OPEN_KEY_PREFIX = 'qunica:conversations:workspace-files-open:'

type WorkspaceFilesOpenUpdater = boolean | ((current: boolean) => boolean)

export interface ConversationChatViewProps {
  conversationId: string
  threadId?: string
  threadGitBranch?: string | null
  threadWorktreePath?: string | null
  workspaceId: string | null
  scope: ConversationScope
  agents: GroupAgentRead[]
  title: React.ReactNode
  subtitle?: React.ReactNode
  announcement?: string | null
  headerActions?: React.ReactNode
  renderHeaderContext?: (disabled: boolean) => React.ReactNode
  capabilities: {
    showAnnouncement: boolean
    showManage: boolean
    showTurnTrace: boolean
    showWorkspace: boolean
    showTerminal?: boolean
    allowMentions: boolean
  }
  onConversationUpdated?: (payload: ConversationUpdatedPayload) => void
  disabledComposerReason?: string
  /**
   * Plain-text names for the conversation and, for a group task, its thread.
   *
   * `title` is a node — a direct chat renders an editable field there — and a
   * notification fires from outside React, so the names it puts in front of the
   * user have to arrive as strings the activity store can hold onto.
   */
  conversationTitle?: string
  threadTitle?: string
  /**
   * Whether the model answering here accepts a reasoning-effort setting.
   * Supplied by the page, which already knows the conversation's agent and
   * provider; resolving it here would make this component fetch on behalf of
   * every caller.
   */
  supportsReasoningEffort?: boolean
  /** Render non-user messages with the dedicated system Assistant identity. */
  agentIsSystem?: boolean
  /** Whether scheduler gaps represent a private moderator-model call. */
  moderatorEnabled?: boolean
  /** Use tighter chrome when the conversation is embedded in a floating panel. */
  compact?: boolean
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
  threadId,
  threadGitBranch,
  threadWorktreePath,
  workspaceId,
  scope,
  agents,
  title,
  subtitle,
  announcement,
  headerActions,
  renderHeaderContext,
  capabilities,
  onConversationUpdated,
  disabledComposerReason,
  conversationTitle,
  threadTitle,
  supportsReasoningEffort,
  agentIsSystem,
  moderatorEnabled,
  compact = false,
}: ConversationChatViewProps) {
  const { t } = useTranslation('chat')
  const showTerminal = capabilities.showTerminal !== false
  const terminal = useOptionalTerminalRuntime()
  useOptionalTerminalConversationRegistration(
    showTerminal ? (threadId ?? conversationId) : undefined,
    workspaceId,
    threadWorktreePath,
  )
  const stateId = threadId ?? conversationId
  const messagesQuery = useConversationMessages(scope, conversationId, threadId)
  const stream = useSendMessageStream(conversationId, {
    scope,
    threadId,
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
      (resume) => resume.state_id === stateId,
    ),
    [activeResumesByMessageId, stateId],
  )
  const isConversationStreaming = stream.isStreaming || activeResumes.length > 0
  const cancelConversationStream = useCallback(() => {
    if (stream.isStreaming) void stream.cancel()
    for (const resume of activeResumes) void resume.cancel()
  }, [activeResumes, stream])

  // What to do with a message typed while the previous reply is still running.
  // `instant` is the original behaviour: the send goes out now and joins the
  // turn in flight. `queue` parks it in the cross-conversation store and
  // releases it once the stream ends, so a follow-up meant as the *next*
  // question does not rewrite the answer being written — and switching to
  // another conversation and back no longer discards it.
  const systemSettings = useSystemSettings()
  const replyInsertMode = systemSettings.data?.reply_insert_mode ?? 'instant'
  const queuedCount = useQueuedMessagesStore(
    (state) => state.byStateId[stateId]?.length ?? 0,
  )
  const enqueueQueued = useQueuedMessagesStore((state) => state.enqueue)
  const clearQueuedMessages = useCallback(
    () => useQueuedMessagesStore.getState().clear(stateId),
    [stateId],
  )
  const sendOrQueueMessage = useCallback(
    (input: MessageSendInput) => {
      if (replyInsertMode !== 'queue' || !isConversationStreaming) return sendMessage(input)
      enqueueQueued(stateId, [input])
    },
    [enqueueQueued, isConversationStreaming, replyInsertMode, sendMessage, stateId],
  )

  // One at a time: releasing the whole queue at once would put every message
  // into a single turn, which is the behaviour queueing exists to avoid. The
  // next release is triggered by this send's own stream ending. The store is
  // read imperatively inside the effect so returning to a conversation that
  // still holds queued messages drains them here without re-render churn.
  useEffect(() => {
    if (isConversationStreaming) return
    const queue = useQueuedMessagesStore.getState()
    const next = queue.beginDispatch(stateId)
    if (!next) return
    let pending: Promise<void>
    try {
      pending = sendMessage(next)
    } catch {
      useQueuedMessagesStore.getState().finishDispatch(stateId, next)
      return
    }
    void pending.then(
      () => useQueuedMessagesStore.getState().finishDispatch(stateId),
      () => useQueuedMessagesStore.getState().finishDispatch(stateId, next),
    )
  }, [isConversationStreaming, sendMessage, stateId])
  const fileNavRequest = useFileNavStore((state) => state.request)
  const registerConversationTitles = useConversationActivityStore(
    (state) => state.registerConversationTitles,
  )
  const setViewedConversation = useConversationActivityStore(
    (state) => state.setViewedConversation,
  )
  const clearViewedConversation = useConversationActivityStore(
    (state) => state.clearViewedConversation,
  )
  const clearActivityFailure = useConversationActivityStore((state) => state.clearFailure)

  useEffect(() => {
    registerConversationTitles(conversationId, threadId, {
      conversation: conversationTitle ?? null,
      thread: threadTitle ?? null,
    })
  }, [conversationId, conversationTitle, registerConversationTitles, threadId, threadTitle])

  // Being on screen is what marks a failure seen and keeps a notification from
  // announcing a reply the user is already watching arrive.
  useEffect(() => {
    setViewedConversation(conversationId, threadId)
    clearActivityFailure(conversationId, threadId)
    return () => clearViewedConversation(conversationId)
  }, [
    clearActivityFailure,
    clearViewedConversation,
    conversationId,
    setViewedConversation,
    threadId,
  ])

  const traceTriggerRef = useRef<HTMLButtonElement | null>(null)
  const [workspaceFilesOpen, setWorkspaceFilesOpen] = useState(() =>
    capabilities.showWorkspace ? readWorkspaceFilesOpen(conversationId) : false,
  )
  const [selectedTurnId, setSelectedTurnId] = useState<string | null>(null)
  const workspaceFilesPane = usePersistentPaneWidth({
    storageKey: 'qunica:layout:workspace-files-pane-width',
    defaultWidth: 280,
    minWidth: 240,
    maxWidth: 560,
  })

  useEffect(() => {
    setWorkspaceFilesOpen(
      capabilities.showWorkspace ? readWorkspaceFilesOpen(conversationId) : false,
    )
    setSelectedTurnId(null)
    clearWarnings(stateId)
  }, [capabilities.showWorkspace, clearWarnings, conversationId, stateId])

  useEffect(() => {
    if (!showTerminal || !terminal) return
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
      void terminal.toggleDock().catch(() => undefined)
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [showTerminal, terminal])

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

  const hint = !agentIsSystem && agents.length === 0 ? t('composer.noAgents') : undefined

  return (
    <div className="flex h-full flex-col">
      <header
        className={cn(
          'flex shrink-0 items-center justify-between border-b border-border/60 bg-background',
          compact ? 'h-11 gap-2 px-3' : 'h-14 gap-4 px-4 lg:px-5',
        )}
      >
        <div className="flex min-w-0 items-center gap-2">
          <h1
            className={cn(
              'font-serif truncate font-semibold tracking-tight',
              compact ? 'text-sm' : 'text-base',
            )}
          >
            {title}
          </h1>
          {renderHeaderContext?.(isConversationStreaming)}
          {subtitle ? <span className="hidden text-xs text-muted-foreground lg:inline">{subtitle}</span> : null}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {scope === 'direct-chats' ? (
            <DirectChatHeaderActions
              key={conversationId}
              chatId={conversationId}
              disabled={isConversationStreaming}
            />
          ) : null}
          {showTerminal && terminal ? (
            <Button
              variant={terminal.isDockOpen ? 'secondary' : 'ghost'}
              size="icon"
              onClick={() => void terminal.toggleDock().catch(() => undefined)}
              aria-label={terminal.isDockOpen ? t('terminal.hide') : t('terminal.show')}
              aria-pressed={terminal.isDockOpen}
            >
              <SquareTerminal className="h-4 w-4" />
            </Button>
          ) : null}
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
          <WorkspaceEditorStage scope={scope} conversationId={conversationId}>
            <div className="flex min-h-0 flex-1 flex-col">
              <MessageList
                groupId={conversationId}
                stateId={stateId}
                threadId={threadId}
                hasOlderMessages={messagesQuery.hasNextPage}
                isLoadingOlderMessages={messagesQuery.isFetchingNextPage}
                onLoadOlderMessages={() => void messagesQuery.fetchNextPage()}
                onSubmitHumanInput={submitHumanInput}
                onViewTurnTrace={capabilities.showTurnTrace ? openTurnTrace : undefined}
                scope={scope}
                agents={agents}
                agentIsSystem={agentIsSystem}
                moderatorEnabled={moderatorEnabled}
              />

              {stream.retry ? (
                <div className="shrink-0 px-4">
                  <div
                    role="status"
                    aria-live="polite"
                    className="mx-auto w-full max-w-6xl rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning-foreground"
                  >
                    {t('stream.reconnecting', {
                      attempt: stream.retry.attempt,
                      max: MAX_RETRY_ATTEMPTS,
                    })}
                  </div>
                </div>
              ) : null}

              {stream.error ? (
                <div className="shrink-0 px-4">
                  <div className="mx-auto w-full max-w-6xl rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
                    {stream.retryExhausted
                      ? t('stream.retryExhausted', { max: MAX_RETRY_ATTEMPTS })
                      : t('stream.error', { message: stream.error })}
                  </div>
                </div>
              ) : null}

              {queuedCount > 0 ? (
                <div className="shrink-0 px-4">
                  <div
                    role="status"
                    aria-live="polite"
                    className="mx-auto flex w-full max-w-6xl items-center justify-between gap-3 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground"
                  >
                    <span>
                      {t('composer.queued', {
                        count: queuedCount,
                        formattedCount: queuedCount,
                      })}
                    </span>
                    <button
                      type="button"
                      className="font-medium text-foreground hover:underline"
                      onClick={clearQueuedMessages}
                    >
                      {t('composer.clearQueued')}
                    </button>
                  </div>
                </div>
              ) : null}

              <Composer
                supportsReasoningEffort={supportsReasoningEffort}
                key={`${scope}:${conversationId}:${threadId ?? 'no-thread'}:${workspaceId ?? 'no-workspace'}`}
                draftKey={`${scope}:${stateId}`}
                conversationId={conversationId}
                workspaceId={workspaceId}
                scope={scope}
                isStreaming={isConversationStreaming}
                onSend={sendOrQueueMessage}
                onCancel={cancelConversationStream}
                hint={hint}
                groupAgents={agents}
                allowMentions={capabilities.allowMentions}
                disabledReason={disabledComposerReason}
                placeholder={agentIsSystem ? t('composer.assistantPlaceholder') : undefined}
                allowConversationDrop={agentIsSystem}
              />
            </div>
          </WorkspaceEditorStage>
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
              threadId={threadId}
              taskBranch={threadGitBranch}
              workspaceId={workspaceId}
              width={workspaceFilesPane.width}
            />
          </>
        ) : null}
      </div>

      {capabilities.showTurnTrace ? (
        <TurnTraceDrawer
          groupId={conversationId}
          agents={agents}
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
