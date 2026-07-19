import { useCallback, useEffect, useRef, useState } from 'react'
import { ArrowUp, ChevronDown, FileText, Image, Paperclip, RotateCw, Square, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { MentionPopover } from '@/components/chat/MentionPopover'
import { ImageLightbox } from '@/components/chat/ImageLightbox'
import { Button } from '@/components/ui/button'
import { getGroupWorkspaceFile, useUploadGroupWorkspaceFiles } from '@/hooks/useGroupFiles'
import { cn } from '@/lib/utils'
import { useAuthStore } from '@/stores/authStore'
import { WORKSPACE_PATHS_MIME, workspacePathsFromDataTransfer } from '@/lib/workspaceDrag'
import type { GroupAgentRead, MessageSendInput } from '@/types/api'

export type WorkspacePathInserter = (paths: string[]) => void

type PendingAttachment = {
  localId: string
  file: File
  status: 'uploading' | 'uploaded' | 'failed'
  uploaded?: { path: string }
  error?: string
}

interface ComposerProps {
  onSend: (input: MessageSendInput) => void
  onCancel?: () => void
  isStreaming?: boolean
  hint?: string
  groupAgents?: GroupAgentRead[]
  groupId?: string
  allowMentions?: boolean
  disabledReason?: string
  onRegisterWorkspacePathInserter?: (insert: WorkspacePathInserter | null) => void
}

/** ~10 lines of text-sm (20px line-height) plus padding. */
const MAX_TEXTAREA_HEIGHT = 208

function errorDetail(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function formatSize(size: number) {
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${Math.round(size / 1024)} KB`
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
}

function PendingAttachmentRow({
  attachment,
  onRemove,
  onRetry,
}: {
  attachment: PendingAttachment
  onRemove: () => void
  onRetry: () => void
}) {
  const { t } = useTranslation('chat')
  const isImage = attachment.file.type.startsWith('image/')
  const [previewUrl, setPreviewUrl] = useState<string | null>(null)
  const [previewOpen, setPreviewOpen] = useState(false)

  useEffect(() => {
    if (!isImage || attachment.file.size === 0) return
    const objectUrl = URL.createObjectURL(attachment.file)
    setPreviewUrl(objectUrl)
    return () => URL.revokeObjectURL(objectUrl)
  }, [attachment.file, isImage])

  return <div className="flex min-w-0 items-center gap-2 rounded-md bg-muted/60 px-2 py-1.5 text-xs">
    {previewUrl ? (
      <button
        type="button"
        className="shrink-0 rounded focus:outline-none focus:ring-2 focus:ring-ring"
        onClick={() => setPreviewOpen(true)}
        aria-label={`Preview ${attachment.file.name}`}
      >
        <img src={previewUrl} alt="" className="h-7 w-7 rounded object-cover" />
      </button>
    ) : isImage ? <Image className="h-4 w-4 shrink-0" /> : <FileText className="h-4 w-4 shrink-0" />}
    <div className="min-w-0 flex-1"><div className="truncate">{attachment.file.name}</div><div className="truncate text-[11px] text-muted-foreground">{attachment.file.type || t('attachments.unknownType')} · {formatSize(attachment.file.size)}</div></div>
    <span className={cn('shrink-0 text-muted-foreground', attachment.status === 'failed' && 'text-destructive')}>{attachment.status === 'failed' ? attachment.error : attachment.status === 'uploading' ? t('attachments.uploading') : t('attachments.uploaded')}</span>
    {attachment.status === 'failed' ? <Button type="button" variant="ghost" size="icon" className="h-6 w-6" onClick={onRetry} aria-label={t('attachments.retryNamed', { name: attachment.file.name })} title={t('attachments.retry')}><RotateCw className="h-3.5 w-3.5" /></Button> : null}
    <Button type="button" variant="ghost" size="icon" className="h-6 w-6" onClick={onRemove} aria-label={t('attachments.removeNamed', { name: attachment.file.name })} title={t('attachments.remove')}><X className="h-3.5 w-3.5" /></Button>
    <ImageLightbox open={previewOpen} onOpenChange={setPreviewOpen} src={previewUrl} alt={attachment.file.name} />
  </div>
}

export function Composer({
  onSend,
  onCancel,
  isStreaming,
  hint,
  groupAgents = [],
  groupId,
  allowMentions = true,
  disabledReason,
  onRegisterWorkspacePathInserter,
}: ComposerProps) {
  const { t } = useTranslation('chat')
  const [value, setValue] = useState('')
  const [attachments, setAttachments] = useState<PendingAttachment[]>([])
  const [uploadError, setUploadError] = useState<string | null>(null)
  const [mentionQuery, setMentionQuery] = useState('')
  const [showMention, setShowMention] = useState(false)
  const [mentionStart, setMentionStart] = useState(-1)
  const [agentSummaryOpen, setAgentSummaryOpen] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const token = useAuthStore((state) => state.token)
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
      if (cleanPaths.length === 0 || !groupId) return
      void Promise.all(cleanPaths.map((path) => getGroupWorkspaceFile(groupId, path, token)))
        .then((files) => {
          const resolved = files.filter((file): file is NonNullable<typeof file> => file !== null)
          if (resolved.length === 0) return
          setAttachments((current) => [
            ...current,
            ...resolved.map((file) => ({
              localId: crypto.randomUUID(),
              file: new File([], file.name),
              status: 'uploaded' as const,
              uploaded: { path: file.path },
            })),
          ])
        })
        .catch((error) => setUploadError(errorDetail(error)))
      setShowMention(false)
    },
    [groupId, token],
  )

  useEffect(() => {
    onRegisterWorkspacePathInserter?.(insertWorkspacePaths)
    return () => onRegisterWorkspacePathInserter?.(null)
  }, [insertWorkspacePaths, onRegisterWorkspacePathInserter])

  const send = () => {
    const content = value.trim()
    const uploaded = attachments.filter((attachment) => attachment.status === 'uploaded' && attachment.uploaded)
    if (!content && uploaded.length === 0) return
    onSend({ content, attachments: uploaded.map((attachment) => ({ path: attachment.uploaded!.path })) })
    setValue('')
    setAttachments((current) => current.filter((attachment) => attachment.status !== 'uploaded'))
    setShowMention(false)
  }

  const uploadFiles = useCallback(
    (fileList: FileList | File[] | null) => {
      const files = Array.from(fileList ?? [])
      if (files.length === 0) return
      setUploadError(null)
      const pending = files.map((file) => ({ localId: crypto.randomUUID(), file, status: 'uploading' as const }))
      setAttachments((current) => [...current, ...pending])
      void (async () => {
        for (const attachment of pending) {
          try {
            const [uploaded] = await uploadWorkspaceFiles.mutateAsync([attachment.file])
            setAttachments((current) => current.map((item) => item.localId === attachment.localId ? { ...item, status: 'uploaded', uploaded } : item))
          } catch (error) {
            setUploadError(errorDetail(error))
            setAttachments((current) => current.map((item) => item.localId === attachment.localId ? { ...item, status: 'failed', error: errorDetail(error) } : item))
          }
        }
        if (fileInputRef.current) fileInputRef.current.value = ''
      })()
    },
    [uploadWorkspaceFiles],
  )

  const retryAttachment = useCallback((attachment: PendingAttachment) => {
    setAttachments((current) => current.map((item) => item.localId === attachment.localId ? { ...item, status: 'uploading', error: undefined } : item))
    void uploadWorkspaceFiles.mutateAsync([attachment.file]).then(([uploaded]) => {
      setAttachments((current) => current.map((item) => item.localId === attachment.localId ? { ...item, status: 'uploaded', uploaded } : item))
    }).catch((error) => {
      setUploadError(errorDetail(error))
      setAttachments((current) => current.map((item) => item.localId === attachment.localId ? { ...item, status: 'failed', error: errorDetail(error) } : item))
    })
  }, [uploadWorkspaceFiles])

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
    if (event.dataTransfer.files.length > 0) {
      event.preventDefault()
      uploadFiles(event.dataTransfer.files)
      return
    }
    const paths = workspacePathsFromDataTransfer(event.dataTransfer)
    if (paths.length === 0) return
    event.preventDefault()
    insertWorkspacePaths(paths)
  }

  const handleDragOver = (event: React.DragEvent<HTMLTextAreaElement>) => {
    const types = Array.from(event.dataTransfer.types)
    if (!types.includes('Files') && !types.includes(WORKSPACE_PATHS_MIME) && !types.includes('text/plain')) {
      return
    }
    event.preventDefault()
    event.dataTransfer.dropEffect = 'copy'
  }

  const handlePaste = (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const files = Array.from(event.clipboardData.items)
      .filter((item) => item.kind === 'file')
      .map((item) => item.getAsFile())
      .filter((file): file is File => file !== null)
    if (files.length > 0) {
      event.preventDefault()
      uploadFiles(files)
    }
  }

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (showMention) return
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      send()
    }
  }

  const hasText = value.trim().length > 0
  const hasUploading = attachments.some((attachment) => attachment.status === 'uploading')
  const hasUploaded = attachments.some((attachment) => attachment.status === 'uploaded')
  const isDisabled = Boolean(disabledReason)
  const showStopAsPrimary = Boolean(isStreaming) && !hasText

  return (
    <div className="shrink-0 px-4 pb-4 pt-1">
      <div className="mx-auto w-full max-w-6xl">
        {uploadError ? <p className="mb-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">{t('errors.uploadDetail', { message: uploadError })}</p> : null}
        {disabledReason ? (
          <p className="mb-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning-foreground">
            {disabledReason}
          </p>
        ) : null}
        <div
          className={cn(
            'relative rounded-2xl border border-border bg-card shadow-sm transition-shadow',
            'focus-within:border-ring/50 focus-within:ring-1 focus-within:ring-ring/40 focus-within:shadow-md',
          )}
        >
          {allowMentions ? (
            <MentionPopover
              agents={groupAgents}
              query={mentionQuery}
              onSelect={handleMentionSelect}
              onClose={() => setShowMention(false)}
              visible={showMention}
            />
          ) : null}
          <textarea
            ref={textareaRef}
            value={value}
            onChange={handleChange}
            onKeyDown={onKeyDown}
            onDrop={handleDrop}
            onDragOver={handleDragOver}
            onPaste={handlePaste}
            placeholder={t('composer.placeholder')}
            rows={1}
            aria-label={t('composer.message')}
            disabled={isDisabled}
            className={cn(
              'block max-h-52 w-full resize-none overflow-y-auto rounded-t-2xl border-0 bg-transparent px-4 pb-1 pt-3.5',
              'text-sm leading-5 text-foreground placeholder:text-muted-foreground/80 focus:outline-none',
            )}
          />
          {attachments.length > 0 ? (
            <div className="space-y-1 px-3 pb-1">
              {attachments.map((attachment) => {
                return <PendingAttachmentRow key={attachment.localId} attachment={attachment} onRetry={() => retryAttachment(attachment)} onRemove={() => setAttachments((current) => current.filter((item) => item.localId !== attachment.localId))} />
              })}
            </div>
          ) : null}
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
              disabled={isDisabled || !groupId || uploadWorkspaceFiles.isPending}
              aria-label={t('composer.upload')}
              title={t('composer.uploadTitle')}
            >
              <Paperclip className="h-4 w-4" />
            </Button>
            {allowMentions && groupAgents.length > 0 ? (
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
                disabled={isDisabled || hasUploading || (!hasText && !hasUploaded)}
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
