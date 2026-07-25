import { useEffect, useRef, useState } from 'react'
import { AlertTriangle, RefreshCw, Save } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Textarea } from '@/components/ui/textarea'
import { useSaveConversationWorkspaceFileText } from '@/hooks/useConversationWorkspaceFiles'
import { ApiError } from '@/lib/api-v2/client'
import { cn } from '@/lib/utils'
import type {
  ConversationScope,
  ConversationWorkspaceFileTextResponse,
} from '@/types/api'

interface WorkspaceTextEditorProps {
  scope: ConversationScope
  conversationId: string
  file: ConversationWorkspaceFileTextResponse
  onRefresh: () => Promise<ConversationWorkspaceFileTextResponse>
}

interface EditorSnapshot {
  path: string
  content: string
  version: string
  truncated: boolean
}

function snapshotFromFile(file: ConversationWorkspaceFileTextResponse): EditorSnapshot {
  return {
    path: file.path,
    content: file.content ?? '',
    version: file.version,
    truncated: file.truncated,
  }
}

function displayError(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function WorkspaceTextEditor({
  scope,
  conversationId,
  file,
  onRefresh,
}: WorkspaceTextEditorProps) {
  const { t } = useTranslation('chat')
  const save = useSaveConversationWorkspaceFileText(scope, conversationId)
  const [snapshot, setSnapshot] = useState<EditorSnapshot>(() => snapshotFromFile(file))
  const [draft, setDraft] = useState(() => file.content ?? '')
  const [conflict, setConflict] = useState(false)
  const [refreshing, setRefreshing] = useState(false)
  const [refreshError, setRefreshError] = useState<string | null>(null)
  const [confirmRefreshOpen, setConfirmRefreshOpen] = useState(false)
  const observedFile = useRef({ path: file.path, version: file.version })
  const currentPath = useRef(file.path)
  const draftRevision = useRef(0)
  currentPath.current = file.path
  const dirty = draft !== snapshot.content

  useEffect(() => {
    const pathChanged = file.path !== observedFile.current.path
    const versionChanged = file.version !== observedFile.current.version
    if (!pathChanged && !versionChanged) return
    observedFile.current = { path: file.path, version: file.version }

    if (!pathChanged && dirty) {
      setConflict(true)
      return
    }

    const next = snapshotFromFile(file)
    setSnapshot(next)
    setDraft(next.content)
    draftRevision.current += 1
    setConflict(false)
    setRefreshError(null)
    save.reset()
  }, [dirty, file, save])

  const applyServerFile = (
    nextFile: ConversationWorkspaceFileTextResponse,
    requestRevision?: number,
  ) => {
    const next = snapshotFromFile(nextFile)
    const preserveDraft = requestRevision !== undefined && draftRevision.current !== requestRevision
    setSnapshot(next)
    if (!preserveDraft) setDraft(next.content)
    setConflict(false)
    setRefreshError(null)
    save.reset()
  }

  const handleSave = async () => {
    if (!dirty || snapshot.truncated) return
    const requestPath = snapshot.path
    const requestRevision = draftRevision.current
    setRefreshError(null)
    setConflict(false)
    save.reset()
    try {
      const saved = await save.mutateAsync({
        path: requestPath,
        content: draft,
        version: snapshot.version,
      })
      if (currentPath.current === requestPath) applyServerFile(saved, requestRevision)
    } catch (error: unknown) {
      if (error instanceof ApiError && error.status === 409) {
        setConflict(true)
      }
    }
  }

  const refreshFromServer = async () => {
    const requestPath = snapshot.path
    const requestRevision = draftRevision.current
    setRefreshing(true)
    setRefreshError(null)
    try {
      const refreshed = await onRefresh()
      if (currentPath.current === requestPath) applyServerFile(refreshed, requestRevision)
    } catch (error: unknown) {
      const message = t('workspace.previewPanel.refreshError', {
        message: displayError(error),
      })
      setRefreshError(message)
      throw new Error(message)
    } finally {
      setRefreshing(false)
    }
  }

  const handleRefresh = () => {
    if (dirty || conflict) {
      setConfirmRefreshOpen(true)
      return
    }
    void refreshFromServer().catch(() => undefined)
  }

  const saveError = save.error
    ? t('workspace.previewPanel.saveError', { message: displayError(save.error) })
    : null

  return (
    <div className="flex min-h-[22rem] flex-col" data-preview-kind="text">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border pb-3">
        <span
          className={cn(
            'rounded-full border px-2 py-1 text-[10px] font-medium uppercase tracking-wide',
            dirty
              ? 'border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300'
              : 'border-border bg-muted/50 text-muted-foreground',
          )}
          role="status"
          aria-live="polite"
        >
          {dirty
            ? t('workspace.previewPanel.unsaved')
            : t('workspace.previewPanel.saved')}
        </span>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-8"
            disabled={save.isPending || refreshing}
            onClick={handleRefresh}
          >
            <RefreshCw
              className={cn('mr-2 h-3.5 w-3.5', refreshing && 'animate-spin')}
              aria-hidden="true"
            />
            {refreshing
              ? t('workspace.previewPanel.refreshing')
              : t('workspace.previewPanel.refresh')}
          </Button>
          <Button
            type="button"
            size="sm"
            className="h-8"
            disabled={!dirty || snapshot.truncated || save.isPending || refreshing}
            onClick={() => void handleSave()}
          >
            <Save className="mr-2 h-3.5 w-3.5" aria-hidden="true" />
            {save.isPending
              ? t('workspace.previewPanel.saving')
              : t('workspace.previewPanel.save')}
          </Button>
        </div>
      </div>

      {snapshot.truncated ? (
        <div className="mt-3 flex gap-2 rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-xs leading-5 text-amber-800 dark:text-amber-200">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
          <p>{t('workspace.previewPanel.truncated')}</p>
        </div>
      ) : null}

      {conflict ? (
        <div className="mt-3 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs leading-5 text-destructive" role="alert">
          <p className="font-medium">{t('workspace.previewPanel.conflictTitle')}</p>
          <p>{t('workspace.previewPanel.conflictDescription')}</p>
        </div>
      ) : saveError ? (
        <p className="mt-3 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive" role="alert">
          {saveError}
        </p>
      ) : null}

      {refreshError ? (
        <p className="mt-3 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive" role="alert">
          {refreshError}
        </p>
      ) : null}

      <label htmlFor="workspace-text-editor" className="sr-only">
        {t('workspace.previewPanel.editorLabel', { name: file.name })}
      </label>
      <Textarea
        id="workspace-text-editor"
        value={draft}
        readOnly={snapshot.truncated || save.isPending || refreshing}
        aria-readonly={snapshot.truncated || save.isPending || refreshing}
        aria-busy={save.isPending || refreshing}
        onChange={(event) => {
          draftRevision.current += 1
          setDraft(event.target.value)
        }}
        spellCheck={false}
        className="mt-3 min-h-[20rem] flex-1 resize-y whitespace-pre font-mono text-xs leading-5"
      />

      <ConfirmDialog
        open={confirmRefreshOpen}
        onOpenChange={setConfirmRefreshOpen}
        title={t('workspace.previewPanel.discardTitle')}
        description={t('workspace.previewPanel.discardDescription')}
        confirmLabel={t('workspace.previewPanel.discardAndRefresh')}
        destructive
        onConfirm={refreshFromServer}
      />
    </div>
  )
}
