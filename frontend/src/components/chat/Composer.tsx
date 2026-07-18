import { useCallback, useEffect, useRef, useState } from 'react'
import { ArrowUp, ChevronDown, Paperclip, Square } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { MentionPopover } from '@/components/chat/MentionPopover'
import { Button } from '@/components/ui/button'
import { useUploadGroupWorkspaceFiles, WorkspaceUploadManyError } from '@/hooks/useGroupFiles'
import { cn } from '@/lib/utils'
import { WORKSPACE_PATHS_MIME, workspacePathsFromDataTransfer } from '@/lib/workspaceDrag'
import type { GroupAgentRead } from '@/types/api'

export type WorkspacePathInserter = (paths: string[]) => void

interface ComposerProps {
  onSend: (content: string) => void
  onCancel?: () => void
  isStreaming?: boolean
  hint?: string
  groupAgents?: GroupAgentRead[]
  groupId?: string
  onRegisterWorkspacePathInserter?: (insert: WorkspacePathInserter | null) => void
}

/** ~10 lines of text-sm (20px line-height) plus padding. */
const MAX_TEXTAREA_HEIGHT = 208

function displayError(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

export function Composer({
  onSend,
  onCancel,
  isStreaming,
  hint,
  groupAgents = [],
  groupId,
  onRegisterWorkspacePathInserter,
}: ComposerProps) {
  const { t } = useTranslation('chat')
  const [value, setValue] = useState('')
  const [uploadError, setUploadError] = useState<string | null>(null)
  const [mentionQuery, setMentionQuery] = useState('')
  const [showMention, setShowMention] = useState(false)
  const [mentionStart, setMentionStart] = useState(-1)
  const [agentSummaryOpen, setAgentSummaryOpen] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const uploadWorkspaceFiles = useUploadGroupWorkspaceFiles(groupId)

  const resizeTextarea = useCallback(() => {
    const textarea = textareaRef.current
    if (!textarea) return
    textarea.style.height = 'auto'
    textarea.style.height = `${Math.min(textarea.scrollHeight, MAX_TEXTAREA_HEIGHT)}px`
  }, [])

  useEffect(() => {
    resizeTextarea()
  }, [value, resizeTextarea])

  const insertWorkspacePaths = useCallback(
    (paths: string[]) => {
      const cleanPaths = paths.map((path) => path.trim()).filter((path) => path.length > 0)
      if (cleanPaths.length === 0) return
      const insertedText = cleanPaths.join('\n')
      const textarea = textareaRef.current
      const start = textarea?.selectionStart ?? value.length
      const end = textarea?.selectionEnd ?? start
      const before = value.slice(0, start)
      const after = value.slice(end)
      const leading = before.length > 0 && !before.endsWith('\n') ? '\n' : ''
      const trailing = after.length > 0 && !after.startsWith('\n') ? '\n' : ''
      const nextValue = `${before}${leading}${insertedText}${trailing}${after}`
      const cursor = before.length + leading.length + insertedText.length
      setValue(nextValue)
      setShowMention(false)
      requestAnimationFrame(() => {
        const target = textareaRef.current
        if (!target) return
        target.setSelectionRange(cursor, cursor)
        target.focus()
      })
    },
    [value],
  )

  useEffect(() => {
    onRegisterWorkspacePathInserter?.(insertWorkspacePaths)
    return () => onRegisterWorkspacePathInserter?.(null)
  }, [insertWorkspacePaths, onRegisterWorkspacePathInserter])

  const send = () => {
    const trimmed = value.trim()
    if (!trimmed) return
    onSend(trimmed)
    setValue('')
    setShowMention(false)
  }

  const uploadFiles = useCallback(
    (fileList: FileList | null) => {
      const files = Array.from(fileList ?? [])
      if (files.length === 0) return
      setUploadError(null)
      void uploadWorkspaceFiles
        .mutateAsync(files)
        .then((uploaded) => {
          insertWorkspacePaths(uploaded.map((file) => file.path))
        })
        .catch((error: unknown) => {
          if (error instanceof WorkspaceUploadManyError && error.uploaded.length > 0) {
            insertWorkspacePaths(error.uploaded.map((file) => file.path))
          }
          setUploadError(displayError(error))
        })
        .finally(() => {
          if (fileInputRef.current) fileInputRef.current.value = ''
        })
    },
    [insertWorkspacePaths, uploadWorkspaceFiles],
  )

  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const newValue = e.target.value
    setValue(newValue)

    const cursorPos = e.target.selectionStart
    const textBeforeCursor = newValue.slice(0, cursorPos)
    const atIdx = textBeforeCursor.lastIndexOf('@')

    if (atIdx >= 0) {
      const beforeAt = atIdx > 0 ? textBeforeCursor[atIdx - 1] : ' '
      if (beforeAt === ' ' || beforeAt === '\n' || atIdx === 0) {
        const query = textBeforeCursor.slice(atIdx + 1)
        if (!query.includes(' ') && !query.includes('\n')) {
          setMentionQuery(query)
          setMentionStart(atIdx)
          setShowMention(true)
          return
        }
      }
    }
    setShowMention(false)
  }

  const handleMentionSelect = useCallback(
    (agent: GroupAgentRead) => {
      const before = value.slice(0, mentionStart)
      const cursorPos = textareaRef.current?.selectionStart ?? value.length
      const after = value.slice(cursorPos)
      const inserted = `@${agent.display_name} `
      const newValue = before + inserted + after
      setValue(newValue)
      setShowMention(false)

      requestAnimationFrame(() => {
        const ta = textareaRef.current
        if (ta) {
          const pos = before.length + inserted.length
          ta.setSelectionRange(pos, pos)
          ta.focus()
        }
      })
    },
    [value, mentionStart],
  )

  const handleDrop = (event: React.DragEvent<HTMLTextAreaElement>) => {
    const paths = workspacePathsFromDataTransfer(event.dataTransfer)
    if (paths.length === 0) return
    event.preventDefault()
    insertWorkspacePaths(paths)
  }

  const handleDragOver = (event: React.DragEvent<HTMLTextAreaElement>) => {
    const types = Array.from(event.dataTransfer.types)
    if (!types.includes(WORKSPACE_PATHS_MIME) && !types.includes('text/plain')) {
      return
    }
    event.preventDefault()
    event.dataTransfer.dropEffect = 'copy'
  }

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (showMention) return
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      send()
    }
  }

  const hasText = value.trim().length > 0
  const showStopAsPrimary = Boolean(isStreaming) && !hasText

  return (
    <div className="shrink-0 px-4 pb-4 pt-1">
      <div className="mx-auto w-full max-w-6xl">
        {uploadError && (
          <p className="mb-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            {uploadError}
          </p>
        )}
        <div
          className={cn(
            'relative rounded-2xl border border-border bg-card shadow-sm transition-shadow',
            'focus-within:border-ring/50 focus-within:ring-1 focus-within:ring-ring/40 focus-within:shadow-md',
          )}
        >
          <MentionPopover
            agents={groupAgents}
            query={mentionQuery}
            onSelect={handleMentionSelect}
            onClose={() => setShowMention(false)}
            visible={showMention}
          />
          <textarea
            ref={textareaRef}
            value={value}
            onChange={handleChange}
            onKeyDown={onKeyDown}
            onDrop={handleDrop}
            onDragOver={handleDragOver}
            placeholder={t('composer.placeholder')}
            rows={1}
            aria-label={t('composer.message')}
            className={cn(
              'block max-h-52 w-full resize-none overflow-y-auto rounded-t-2xl border-0 bg-transparent px-4 pb-1 pt-3.5',
              'text-sm leading-5 text-foreground placeholder:text-muted-foreground/80 focus:outline-none',
            )}
          />
          <div className="flex items-center gap-1.5 px-2.5 pb-2.5 pt-1">
            <input
              ref={fileInputRef}
              type="file"
              multiple
              className="sr-only"
              onChange={(event) => uploadFiles(event.target.files)}
              aria-label={t('composer.upload')}
            />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-8 w-8 rounded-full text-muted-foreground hover:text-foreground"
              onClick={() => fileInputRef.current?.click()}
              disabled={!groupId || uploadWorkspaceFiles.isPending}
              aria-label={t('composer.upload')}
              title={t('composer.uploadTitle')}
            >
              <Paperclip className="h-4 w-4" />
            </Button>
            {groupAgents.length > 0 ? (
              <div className="relative min-w-0 flex-1">
                <div className="flex min-w-0 items-center gap-1 px-1 text-[11px] text-muted-foreground">
                  {groupAgents.slice(0, 3).map((agent) => (
                    <span key={agent.id} className="truncate whitespace-nowrap">
                      @{agent.display_name}
                    </span>
                  ))}
                  {groupAgents.length > 3 ? (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-6 shrink-0 gap-0.5 px-1 text-[11px]"
                      onClick={() => setAgentSummaryOpen((open) => !open)}
                      aria-expanded={agentSummaryOpen}
                      aria-label={t('composer.showMore', { count: groupAgents.length - 3 })}
                    >
                      +{groupAgents.length - 3}
                      <ChevronDown className="h-3 w-3" />
                    </Button>
                  ) : null}
                </div>
                {agentSummaryOpen ? (
                  <>
                    <button
                      type="button"
                      className="fixed inset-0 z-40 cursor-default"
                      aria-label={t('composer.closeAgents')}
                      onClick={() => setAgentSummaryOpen(false)}
                    />
                    <div className="absolute bottom-full left-0 z-50 mb-2 max-h-48 w-64 overflow-y-auto rounded-md border border-border bg-background p-1 shadow-md">
                      {groupAgents.map((agent) => (
                        <div key={agent.id} className="truncate px-2 py-1.5 text-xs text-foreground">
                          @{agent.display_name}
                        </div>
                      ))}
                    </div>
                  </>
                ) : null}
              </div>
            ) : hint ? (
              <p className="min-w-0 flex-1 truncate px-1 text-[11px] text-muted-foreground">
                {hint}
              </p>
            ) : (
              <div className="flex-1" />
            )}
            {isStreaming && hasText && (
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="h-8 w-8 shrink-0 rounded-full"
                onClick={onCancel}
                aria-label={t('composer.stop')}
                title={t('composer.stopTitle')}
              >
                <Square className="h-3.5 w-3.5" />
              </Button>
            )}
            {showStopAsPrimary ? (
              <Button
                type="button"
                size="icon"
                className="h-8 w-8 shrink-0 rounded-full"
                onClick={onCancel}
                aria-label={t('composer.stop')}
                title={t('composer.stopTitle')}
              >
                <Square className="h-3.5 w-3.5" />
              </Button>
            ) : (
              <Button
                type="button"
                size="icon"
                className="h-8 w-8 shrink-0 rounded-full"
                onClick={send}
                disabled={!hasText}
                aria-label={t('composer.send')}
                title={t('composer.sendTitle')}
              >
                <ArrowUp className="h-4 w-4" />
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
