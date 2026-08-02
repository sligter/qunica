import { useCallback, useEffect, useRef, useState } from 'react'
import { ArrowUp, ChevronDown, FileText, Image, Paperclip, RotateCw, Square, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { MentionPopover } from '@/components/chat/MentionPopover'
import { ImageLightbox } from '@/components/chat/ImageLightbox'
import { Button } from '@/components/ui/button'
import {
  getConversationWorkspaceFile,
  getConversationWorkspaceFileMetadata,
  type ConversationWorkspaceFileMetadata,
} from '@/hooks/useConversationWorkspaceFiles'
import { useUploadGroupWorkspaceFiles } from '@/hooks/useGroupFiles'
import {
  workspaceErrorMessageKey,
  type WorkspaceErrorMessageKey,
} from '@/i18n/localizedError'
import { cn } from '@/lib/utils'
import { useAuthStore } from '@/stores/authStore'
import {
  isWorkspaceRelativePath,
  WORKSPACE_ITEM_MIME,
  workspaceItemsFromDataTransfer,
  workspacePathsFromDataTransfer,
} from '@/lib/workspaceDrag'
import type {
  ConversationScope,
  GroupAgentRead,
  MessageSendInput,
} from '@/types/api'

type UploadAttachment = {
  kind: 'upload'
  localId: string
  file: File
  status: 'uploading' | 'uploaded' | 'failed'
  uploaded?: { path: string }
}

export interface WorkspaceAttachment {
  kind: 'workspace'
  localId: string
  path: string
  name: string
  mime: string
  size: number
}

type PendingAttachment = UploadAttachment | WorkspaceAttachment

type ComposerNoticeKey =
  | 'composer.drop.ready'
  | 'composer.drop.fileAdded'
  | 'composer.drop.directoryInserted'
  | 'composer.drop.unsupported'
  | 'composer.drop.uploadUnsupported'
  | 'composer.drop.noWorkspace'
  | 'composer.drop.unreadable'
  | 'composer.drop.limitReached'

type ComposerNotice = {
  key: ComposerNoticeKey
  tone: 'status' | 'error'
  values?: Record<string, number | string>
}

interface ComposerProps {
  onSend: (input: MessageSendInput) => void | Promise<void>
  onCancel?: () => void
  isStreaming?: boolean
  hint?: string
  groupAgents?: GroupAgentRead[]
  conversationId?: string
  workspaceId?: string | null
  scope?: ConversationScope
  /** Compatibility for direct Composer consumers while callers migrate to conversationId. */
  groupId?: string
  allowMentions?: boolean
  disabledReason?: string
  /**
   * Models selectable for this message. Empty or single-entry hides the
   * picker: a group whose agents span several providers has no one catalog to
   * offer, and one model is not a choice.
   */
  models?: Array<{ id: string; supports_reasoning_effort?: boolean }>
  /** The model used when the user has not picked one. */
  defaultModel?: string
}

/** Reasoning depth, matching the backend's three levels. */
const EFFORT_LEVELS = ['low', 'medium', 'high'] as const
type EffortLevel = (typeof EFFORT_LEVELS)[number]

/** ~10 lines of text-sm (20px line-height) plus padding. */
const MAX_TEXTAREA_HEIGHT = 208
const MAX_ATTACHMENTS_PER_MESSAGE = 10

function formatSize(size: number) {
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${Math.round(size / 1024)} KB`
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
}

function attachmentPath(attachment: PendingAttachment): string | null {
  if (attachment.kind === 'workspace') return attachment.path
  return attachment.status === 'uploaded' ? attachment.uploaded?.path ?? null : null
}

function attachmentDetails(attachment: PendingAttachment) {
  if (attachment.kind === 'workspace') {
    return {
      name: attachment.name,
      mime: attachment.mime,
      size: attachment.size,
    }
  }
  return {
    name: attachment.file.name,
    mime: attachment.file.type,
    size: attachment.file.size,
  }
}

function isRecognizedDrop(dataTransfer: DataTransfer): boolean {
  const types = Array.from(dataTransfer.types)
  return (dataTransfer.files?.length ?? 0) > 0
    || types.includes('Files')
    || types.includes(WORKSPACE_ITEM_MIME)
    || types.includes('text/plain')
}

function PendingAttachmentRow({
  attachment,
  onRemove,
  onRetry,
  retryDisabled,
}: {
  attachment: PendingAttachment
  onRemove: () => void
  onRetry: () => void
  retryDisabled: boolean
}) {
  const { t } = useTranslation('chat')
  const details = attachmentDetails(attachment)
  const previewFile = attachment.kind === 'upload' ? attachment.file : null
  const isImage = details.mime.startsWith('image/')
  const [previewUrl, setPreviewUrl] = useState<string | null>(null)
  const [previewOpen, setPreviewOpen] = useState(false)

  useEffect(() => {
    if (!isImage || !previewFile || previewFile.size === 0) return
    const objectUrl = URL.createObjectURL(previewFile)
    setPreviewUrl(objectUrl)
    return () => URL.revokeObjectURL(objectUrl)
  }, [isImage, previewFile])

  return <div className="flex min-w-0 items-center gap-2 rounded-md bg-muted/60 px-2 py-1.5 text-xs">
    {previewUrl ? (
      <button
        type="button"
        className="shrink-0 rounded focus:outline-none focus:ring-2 focus:ring-ring"
        onClick={() => setPreviewOpen(true)}
        aria-label={t('attachments.previewNamed', { name: details.name })}
      >
        <img src={previewUrl} alt="" className="h-7 w-7 rounded object-cover" />
      </button>
    ) : isImage ? <Image className="h-4 w-4 shrink-0" /> : <FileText className="h-4 w-4 shrink-0" />}
    <div className="min-w-0 flex-1"><div className="truncate">{details.name}</div><div className="truncate text-2xs text-muted-foreground">{details.mime || t('attachments.unknownType')} · {formatSize(details.size)}</div></div>
    <span className={cn('shrink-0 text-muted-foreground', attachment.kind === 'upload' && attachment.status === 'failed' && 'text-destructive')}>{attachment.kind === 'workspace' ? t('attachments.workspace') : attachment.status === 'failed' ? t('attachments.failed') : attachment.status === 'uploading' ? t('attachments.uploading') : t('attachments.uploaded')}</span>
    {attachment.kind === 'upload' && attachment.status === 'failed' ? <Button type="button" variant="ghost" size="icon" className="h-6 w-6" onClick={onRetry} disabled={retryDisabled} aria-label={t('attachments.retryNamed', { name: details.name })} title={t('attachments.retry')}><RotateCw className="h-3.5 w-3.5" /></Button> : null}
    <Button type="button" variant="ghost" size="icon" className="h-6 w-6" onClick={onRemove} aria-label={t('attachments.removeNamed', { name: details.name })} title={t('attachments.remove')}><X className="h-3.5 w-3.5" /></Button>
    <ImageLightbox open={previewOpen} onOpenChange={setPreviewOpen} src={previewUrl} alt={details.name} />
  </div>
}

export function Composer({
  onSend,
  onCancel,
  isStreaming,
  hint,
  groupAgents = [],
  conversationId,
  workspaceId,
  scope = 'groups',
  groupId,
  allowMentions = true,
  disabledReason,
  models = [],
  defaultModel,
}: ComposerProps) {
  const { t } = useTranslation('chat')
  const [value, setValue] = useState('')
  const [attachments, setAttachmentState] = useState<PendingAttachment[]>([])
  const [uploadError, setUploadError] = useState<WorkspaceErrorMessageKey | null>(null)
  const [notice, setNotice] = useState<ComposerNotice | null>(null)
  const [isDragActive, setIsDragActive] = useState(false)
  const [isSending, setIsSending] = useState(false)
  const [mentionQuery, setMentionQuery] = useState('')
  const [showMention, setShowMention] = useState(false)
  // null means "whatever the agent is configured with"; only an explicit pick
  // becomes an override on the wire.
  const [modelOverride, setModelOverride] = useState<string | null>(null)
  const [showModels, setShowModels] = useState(false)
  const selectedModel = modelOverride ?? defaultModel ?? models[0]?.id ?? ''
  const [effortOverride, setEffortOverride] = useState<EffortLevel | null>(null)
  const [showEfforts, setShowEfforts] = useState(false)
  // Only offered when the selected model advertises support. Sending the field
  // to a model that rejects it turns a normal question into a provider error.
  const effortSupported = Boolean(
    models.find((model) => model.id === selectedModel)?.supports_reasoning_effort,
  )
  const [mentionStart, setMentionStart] = useState(-1)
  const [agentSummaryOpen, setAgentSummaryOpen] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const attachmentsRef = useRef<PendingAttachment[]>([])
  const valueRef = useRef('')
  const draftRevisionRef = useRef(0)
  const dragDepthRef = useRef(0)
  const token = useAuthStore((state) => state.token)
  const resolvedConversationId = conversationId ?? groupId
  const hasWorkspace = workspaceId === undefined
    ? Boolean(resolvedConversationId)
    : Boolean(workspaceId)
  const isDisabled = Boolean(disabledReason)
  const uploadWorkspaceFiles = useUploadGroupWorkspaceFiles(
    scope === 'groups' ? resolvedConversationId : undefined,
  )

  const updateValue = useCallback((next: string) => {
    draftRevisionRef.current += 1
    valueRef.current = next
    setValue(next)
  }, [])

  const updateAttachments = useCallback(
    (update: (current: PendingAttachment[]) => PendingAttachment[]) => {
      const next = update(attachmentsRef.current)
      attachmentsRef.current = next
      setAttachmentState(next)
    },
    [],
  )

  const showNotice = useCallback(
    (
      key: ComposerNoticeKey,
      tone: ComposerNotice['tone'] = 'status',
      values?: ComposerNotice['values'],
    ) => setNotice({ key, tone, values }),
    [],
  )

  const ensureWorkspaceContext = useCallback(() => {
    if (resolvedConversationId && hasWorkspace) return true
    showNotice('composer.drop.noWorkspace', 'error')
    return false
  }, [hasWorkspace, resolvedConversationId, showNotice])

  const resizeTextarea = useCallback(() => {
    const textarea = textareaRef.current
    if (!textarea) return
    textarea.style.height = 'auto'
    textarea.style.height = `${Math.min(textarea.scrollHeight, MAX_TEXTAREA_HEIGHT)}px`
  }, [])

  useEffect(() => {
    resizeTextarea()
  }, [value, resizeTextarea])

  const insertDirectoryPaths = useCallback((paths: string[]) => {
    const cleanPaths = Array.from(new Set(
      paths.map((path) => path.trim()).filter((path) => path.length > 0),
    ))
    if (cleanPaths.length === 0) return

    const textarea = textareaRef.current
    const draft = valueRef.current
    const hasFocus = textarea !== null && document.activeElement === textarea
    const start = hasFocus ? textarea.selectionStart : draft.length
    const end = hasFocus ? textarea.selectionEnd : draft.length
    const before = draft.slice(0, start)
    const after = draft.slice(end)
    const inserted = cleanPaths.join(' ')
    const leadingSpace = before.length > 0 && !/\s$/.test(before) ? ' ' : ''
    const trailingSpace = after.length > 0 && !/^\s/.test(after) ? ' ' : ''
    const next = `${before}${leadingSpace}${inserted}${trailingSpace}${after}`
    const cursor = before.length + leadingSpace.length + inserted.length + trailingSpace.length

    updateValue(next)
    setShowMention(false)
    showNotice('composer.drop.directoryInserted', 'status', { count: cleanPaths.length })
    requestAnimationFrame(() => {
      const current = textareaRef.current
      if (!current) return
      current.setSelectionRange(cursor, cursor)
      current.focus()
    })
  }, [showNotice, updateValue])

  const addWorkspaceMetadata = useCallback(
    (metadata: ConversationWorkspaceFileMetadata[]) => {
      let addedCount = 0
      let limitReached = false
      updateAttachments((current) => {
        const existingPaths = new Set(
          current.map(attachmentPath).filter((path): path is string => path !== null),
        )
        const additions: WorkspaceAttachment[] = []
        for (const file of metadata) {
          if (existingPaths.has(file.path)) continue
          if (current.length + additions.length >= MAX_ATTACHMENTS_PER_MESSAGE) {
            limitReached = true
            break
          }
          existingPaths.add(file.path)
          additions.push({
            kind: 'workspace',
            localId: crypto.randomUUID(),
            path: file.path,
            name: file.name,
            mime: file.mime_type,
            size: file.size,
          })
        }
        addedCount = additions.length
        return additions.length > 0 ? [...current, ...additions] : current
      })

      if (addedCount > 0) {
        showNotice('composer.drop.fileAdded', 'status', { count: addedCount })
      } else if (limitReached) {
        showNotice('composer.drop.limitReached', 'error', {
          count: MAX_ATTACHMENTS_PER_MESSAGE,
        })
      }
      return addedCount
    },
    [showNotice, updateAttachments],
  )

  const addWorkspaceFiles = useCallback(async (paths: string[]) => {
    if (!ensureWorkspaceContext() || !resolvedConversationId) return
    const existingPaths = new Set(
      attachmentsRef.current.map(attachmentPath).filter((path): path is string => path !== null),
    )
    const cleanPaths = Array.from(new Set(
      paths
        .map((path) => path.trim())
        .filter((path) => path.length > 0 && !existingPaths.has(path)),
    ))
    if (cleanPaths.length === 0) return
    if (attachmentsRef.current.length >= MAX_ATTACHMENTS_PER_MESSAGE) {
      showNotice('composer.drop.limitReached', 'error', {
        count: MAX_ATTACHMENTS_PER_MESSAGE,
      })
      return
    }

    setUploadError(null)
    const results = await Promise.allSettled(
      cleanPaths.map((path) => getConversationWorkspaceFileMetadata(
        scope,
        resolvedConversationId,
        path,
        token,
      )),
    )
    const metadata = results
      .filter((result): result is PromiseFulfilledResult<ConversationWorkspaceFileMetadata> => (
        result.status === 'fulfilled'
      ))
      .map((result) => result.value)
    addWorkspaceMetadata(metadata)
    if (results.some((result) => result.status === 'rejected')) {
      showNotice('composer.drop.unreadable', 'error')
    }
  }, [addWorkspaceMetadata, ensureWorkspaceContext, resolvedConversationId, scope, showNotice, token])

  const verifyAndInsertDirectories = useCallback(async (paths: string[]) => {
    if (!ensureWorkspaceContext() || !resolvedConversationId) return
    const cleanPaths = Array.from(new Set(
      paths.map((path) => path.trim()).filter((path) => path.length > 0),
    ))
    if (cleanPaths.length === 0) return
    const results = await Promise.allSettled(
      cleanPaths.map((path) => getConversationWorkspaceFile(
        scope,
        resolvedConversationId,
        path,
        token,
      )),
    )
    const directories = results.flatMap((result) => (
      result.status === 'fulfilled' && result.value?.is_dir ? [result.value.path] : []
    ))
    if (directories.length > 0) insertDirectoryPaths(directories)
    if (directories.length === 0 && results.length > 0) {
      showNotice('composer.drop.unreadable', 'error')
    }
  }, [ensureWorkspaceContext, insertDirectoryPaths, resolvedConversationId, scope, showNotice, token])

  const insertWorkspacePaths = useCallback((paths: string[]) => {
    setShowMention(false)
    void (async () => {
      if (!ensureWorkspaceContext() || !resolvedConversationId) return
      const cleanPaths = Array.from(new Set(
        paths.map((path) => path.trim()).filter(isWorkspaceRelativePath),
      )).slice(0, MAX_ATTACHMENTS_PER_MESSAGE)
      const results = await Promise.allSettled(
        cleanPaths.map((path) => getConversationWorkspaceFile(
          scope,
          resolvedConversationId,
          path,
          token,
        )),
      )
      const files: string[] = []
      const directories: string[] = []
      for (const result of results) {
        if (result.status !== 'fulfilled' || result.value === null) continue
        if (result.value.is_dir) directories.push(result.value.path)
        else files.push(result.value.path)
      }
      if (directories.length > 0) insertDirectoryPaths(directories)
      if (files.length > 0) await addWorkspaceFiles(files)
      if (files.length === 0 && directories.length === 0 && cleanPaths.length > 0) {
        showNotice('composer.drop.unreadable', 'error')
      }
    })()
  }, [addWorkspaceFiles, ensureWorkspaceContext, insertDirectoryPaths, resolvedConversationId, scope, showNotice, token])

  const send = async () => {
    if (isSending) return
    if (attachmentsRef.current.some((attachment) => (
      attachment.kind === 'upload' && attachment.status === 'uploading'
    ))) return
    const content = valueRef.current.trim()
    const ready = attachmentsRef.current.flatMap((attachment) => {
      const path = attachmentPath(attachment)
      return path === null ? [] : [{ localId: attachment.localId, path }]
    })
    if (!content && ready.length === 0) return

    const sentIds = new Set(ready.map((attachment) => attachment.localId))
    const sentDraftRevision = draftRevisionRef.current
    setIsSending(true)
    try {
      await onSend({
        content,
        attachments: ready.map((attachment) => ({ path: attachment.path })),
        // Omitted unless the user picked a different model. Always sending the
        // current default would pin the message to whatever the agent happened
        // to be set to at send time.
        ...(modelOverride ? { model_override: modelOverride } : {}),
        ...(effortOverride && effortSupported ? { effort_override: effortOverride } : {}),
      })
      if (draftRevisionRef.current === sentDraftRevision) updateValue('')
      updateAttachments((current) => current.filter((attachment) => !sentIds.has(attachment.localId)))
      setShowMention(false)
    } catch {
      // Keep the draft and ready attachments intact so the user can retry.
    } finally {
      setIsSending(false)
    }
  }

  const uploadFiles = useCallback(
    (fileList: FileList | File[] | null) => {
      const files = Array.from(fileList ?? [])
      if (files.length === 0) return
      setUploadError(null)
      if (isDisabled) return
      if (!ensureWorkspaceContext()) {
        if (fileInputRef.current) fileInputRef.current.value = ''
        return
      }
      if (scope !== 'groups') {
        showNotice('composer.drop.uploadUnsupported', 'error')
        if (fileInputRef.current) fileInputRef.current.value = ''
        return
      }
      const capacity = MAX_ATTACHMENTS_PER_MESSAGE - attachmentsRef.current.length
      const acceptedFiles = files.slice(0, Math.max(0, capacity))
      if (acceptedFiles.length === 0) {
        showNotice('composer.drop.limitReached', 'error', {
          count: MAX_ATTACHMENTS_PER_MESSAGE,
        })
        if (fileInputRef.current) fileInputRef.current.value = ''
        return
      }
      const pending: UploadAttachment[] = acceptedFiles.map((file) => ({
        kind: 'upload',
        localId: crypto.randomUUID(),
        file,
        status: 'uploading',
      }))
      updateAttachments((current) => [...current, ...pending])
      if (acceptedFiles.length < files.length) {
        showNotice('composer.drop.limitReached', 'error', {
          count: MAX_ATTACHMENTS_PER_MESSAGE,
        })
      }
      void (async () => {
        for (const attachment of pending) {
          try {
            const [uploaded] = await uploadWorkspaceFiles.mutateAsync([attachment.file])
            if (!uploaded) throw new Error('Workspace upload returned no file')
            updateAttachments((current) => current.map((item) => (
              item.kind === 'upload' && item.localId === attachment.localId
                ? { ...item, status: 'uploaded' as const, uploaded: { path: uploaded.path } }
                : item
            )))
            showNotice('composer.drop.fileAdded', 'status', { count: 1 })
          } catch (error) {
            setUploadError(workspaceErrorMessageKey(error))
            updateAttachments((current) => current.map((item) => (
              item.kind === 'upload' && item.localId === attachment.localId
                ? { ...item, status: 'failed' as const }
                : item
            )))
          }
        }
        if (fileInputRef.current) fileInputRef.current.value = ''
      })()
    },
    [ensureWorkspaceContext, isDisabled, scope, showNotice, updateAttachments, uploadWorkspaceFiles],
  )

  const retryAttachment = useCallback((attachment: PendingAttachment) => {
    if (isDisabled || attachment.kind !== 'upload' || scope !== 'groups') return
    setUploadError(null)
    updateAttachments((current) => current.map((item) => (
      item.kind === 'upload' && item.localId === attachment.localId
        ? { ...item, status: 'uploading' as const }
        : item
    )))
    void uploadWorkspaceFiles.mutateAsync([attachment.file]).then(([uploaded]) => {
      if (!uploaded) throw new Error('Workspace upload returned no file')
      updateAttachments((current) => current.map((item) => (
        item.kind === 'upload' && item.localId === attachment.localId
          ? { ...item, status: 'uploaded' as const, uploaded: { path: uploaded.path } }
          : item
      )))
    }).catch((error) => {
      setUploadError(workspaceErrorMessageKey(error))
      updateAttachments((current) => current.map((item) => (
        item.kind === 'upload' && item.localId === attachment.localId
          ? { ...item, status: 'failed' as const }
          : item
      )))
    })
  }, [isDisabled, scope, updateAttachments, uploadWorkspaceFiles])

  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const newValue = e.target.value
    updateValue(newValue)

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
      const draft = valueRef.current
      const before = draft.slice(0, mentionStart)
      const cursorPos = textareaRef.current?.selectionStart ?? draft.length
      const after = draft.slice(cursorPos)
      const inserted = `@${agent.display_name} `
      const newValue = before + inserted + after
      updateValue(newValue)
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
    [mentionStart, updateValue],
  )

  const resetDragState = useCallback(() => {
    dragDepthRef.current = 0
    setIsDragActive(false)
    setNotice((current) => current?.key === 'composer.drop.ready' ? null : current)
  }, [])

  const handleDragEnter = (event: React.DragEvent<HTMLDivElement>) => {
    if (!isRecognizedDrop(event.dataTransfer)) return
    event.preventDefault()
    if (isDisabled) {
      event.dataTransfer.dropEffect = 'none'
      return
    }
    dragDepthRef.current += 1
    if (dragDepthRef.current === 1) {
      setIsDragActive(true)
      showNotice('composer.drop.ready')
    }
  }

  const handleDragLeave = (event: React.DragEvent<HTMLDivElement>) => {
    if (dragDepthRef.current === 0) return
    event.preventDefault()
    dragDepthRef.current = Math.max(0, dragDepthRef.current - 1)
    if (dragDepthRef.current === 0) {
      setIsDragActive(false)
      setNotice((current) => current?.key === 'composer.drop.ready' ? null : current)
    }
  }

  const handleDragOver = (event: React.DragEvent<HTMLDivElement>) => {
    if (!isRecognizedDrop(event.dataTransfer)) return
    event.preventDefault()
    if (isDisabled) {
      event.dataTransfer.dropEffect = 'none'
      return
    }
    event.dataTransfer.dropEffect = 'copy'
  }

  const handleDrop = (event: React.DragEvent<HTMLDivElement>) => {
    resetDragState()
    event.preventDefault()
    if (isDisabled) {
      event.dataTransfer.dropEffect = 'none'
      return
    }

    const workspaceItems = workspaceItemsFromDataTransfer(event.dataTransfer)
    if (workspaceItems.files.length > 0 || workspaceItems.directories.length > 0) {
      if (workspaceItems.files.length > 0) {
        void addWorkspaceFiles(workspaceItems.files.map((item) => item.path))
      }
      if (workspaceItems.directories.length > 0) {
        void verifyAndInsertDirectories(workspaceItems.directories.map((item) => item.path))
      }
      return
    }

    const files = Array.from(event.dataTransfer.files)
    if (files.length > 0) {
      uploadFiles(files)
      return
    }
    const fallbackPaths = workspacePathsFromDataTransfer(event.dataTransfer)
    if (fallbackPaths.length > 0) {
      insertWorkspacePaths(fallbackPaths)
      return
    }
    showNotice('composer.drop.unsupported', 'error')
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
      void send()
    }
  }

  const hasText = value.trim().length > 0
  const hasUploading = attachments.some((attachment) => (
    attachment.kind === 'upload' && attachment.status === 'uploading'
  ))
  const hasUploaded = attachments.some((attachment) => attachmentPath(attachment) !== null)
  const showStopAsPrimary = Boolean(isStreaming) && !hasText
  const noticeText = notice ? t(notice.key, notice.values ?? {}) : ''
  const liveStatus = notice?.tone === 'status' ? noticeText : ''

  return (
    <div className="shrink-0 px-4 pb-4 pt-1">
      <div className="mx-auto w-full max-w-6xl">
        {uploadError ? <p role="alert" aria-live="polite" className="mb-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">{t('errors.uploadDetail', { message: t(uploadError) })}</p> : null}
        {notice?.tone === 'error' ? <p role="alert" aria-live="polite" className="mb-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">{noticeText}</p> : null}
        {disabledReason ? (
          <p className="mb-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning-foreground">
            {disabledReason}
          </p>
        ) : null}
        <p id="composer-drop-status" className="sr-only" aria-live="polite" aria-atomic="true">
          {liveStatus}
        </p>
        <div
          role="group"
          aria-label={t('composer.dropZone')}
          aria-disabled={isDisabled}
          data-drop-active={isDragActive ? 'true' : 'false'}
          onDragEnter={handleDragEnter}
          onDragLeave={handleDragLeave}
          onDragOver={handleDragOver}
          onDrop={handleDrop}
          className={cn(
            'relative rounded-2xl border border-border bg-card shadow-sm transition-[border-color,background-color,box-shadow]',
            'focus-within:border-ring/50 focus-within:ring-1 focus-within:ring-ring/40 focus-within:shadow-md',
            isDragActive && 'cursor-copy border-primary/70 bg-primary/5 shadow-md ring-2 ring-primary/20',
            isDisabled && 'cursor-not-allowed',
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
            onPaste={handlePaste}
            placeholder={t('composer.placeholder')}
            rows={1}
            aria-label={t('composer.message')}
            aria-describedby="composer-drop-status"
            disabled={isDisabled}
            className={cn(
              'block max-h-52 w-full resize-none overflow-y-auto rounded-t-2xl border-0 bg-transparent px-4 pb-1 pt-3.5',
              'text-sm leading-5 text-foreground placeholder:text-muted-foreground/80 focus:outline-none',
            )}
          />
          {attachments.length > 0 ? (
            <div className="space-y-1 px-3 pb-1">
              {attachments.map((attachment) => {
                return <PendingAttachmentRow key={attachment.localId} attachment={attachment} onRetry={() => retryAttachment(attachment)} retryDisabled={isDisabled} onRemove={() => updateAttachments((current) => current.filter((item) => item.localId !== attachment.localId))} />
              })}
            </div>
          ) : null}
          <div className="flex items-center gap-1.5 px-2.5 pb-2.5 pt-1">
            {hasWorkspace ? (
              <>
                <input
                  ref={fileInputRef}
                  type="file"
                  multiple
                  className="sr-only"
                  onChange={(event) => uploadFiles(event.target.files)}
                  disabled={isDisabled}
                  aria-label={t('composer.upload')}
                  aria-describedby="composer-drop-status"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 rounded-full text-muted-foreground hover:text-foreground"
                  onClick={() => {
                    setUploadError(null)
                    if (!ensureWorkspaceContext()) return
                    if (scope !== 'groups') {
                      showNotice('composer.drop.uploadUnsupported', 'error')
                      return
                    }
                    fileInputRef.current?.click()
                  }}
                  disabled={isDisabled || uploadWorkspaceFiles.isPending}
                  aria-label={t('composer.upload')}
                  title={scope === 'groups' ? t('composer.uploadTitle') : t('composer.uploadUnsupportedTitle')}
                >
                  <Paperclip className="h-4 w-4" />
                </Button>
              </>
            ) : null}
            {allowMentions && groupAgents.length > 0 ? (
              <div className="relative min-w-0 flex-1">
                <div className="flex min-w-0 items-center gap-1 px-1 text-2xs text-muted-foreground">
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
                      className="h-6 shrink-0 gap-0.5 px-1 text-2xs"
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
              <p className="min-w-0 flex-1 truncate px-1 text-2xs text-muted-foreground">
                {hint}
              </p>
            ) : (
              <div className="flex-1" />
            )}
            {effortSupported ? (
              <div className="relative shrink-0">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7 gap-1 px-2 text-2xs text-muted-foreground"
                  onClick={() => setShowEfforts((open) => !open)}
                  aria-haspopup="listbox"
                  aria-expanded={showEfforts}
                >
                  <span className="truncate">
                    {effortOverride
                      ? t(`composer.effort.${effortOverride}`)
                      : t('composer.effort.label')}
                  </span>
                  <ChevronDown className="h-3 w-3 shrink-0" aria-hidden />
                </Button>
                {showEfforts ? (
                  <>
                    <div
                      className="fixed inset-0 z-40 cursor-default"
                      onClick={() => setShowEfforts(false)}
                    />
                    <div
                      role="listbox"
                      className="absolute bottom-full right-0 z-50 mb-2 w-40 rounded-md border border-border bg-background p-1 shadow-md"
                    >
                      {EFFORT_LEVELS.map((level) => (
                        <button
                          key={level}
                          type="button"
                          role="option"
                          aria-selected={level === effortOverride}
                          className="flex w-full items-center rounded px-2 py-1.5 text-left text-xs hover:bg-accent"
                          onClick={() => {
                            setEffortOverride(level)
                            setShowEfforts(false)
                          }}
                        >
                          {t(`composer.effort.${level}`)}
                        </button>
                      ))}
                    </div>
                  </>
                ) : null}
              </div>
            ) : null}
            {models.length > 1 ? (
              <div className="relative shrink-0">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7 max-w-40 gap-1 px-2 text-2xs text-muted-foreground"
                  onClick={() => setShowModels((open) => !open)}
                  aria-haspopup="listbox"
                  aria-expanded={showModels}
                >
                  <span className="truncate">{selectedModel}</span>
                  <ChevronDown className="h-3 w-3 shrink-0" aria-hidden />
                </Button>
                {showModels ? (
                  <>
                    <div
                      className="fixed inset-0 z-40 cursor-default"
                      onClick={() => setShowModels(false)}
                    />
                    <div
                      role="listbox"
                      className="absolute bottom-full right-0 z-50 mb-2 max-h-48 w-56 overflow-y-auto rounded-md border border-border bg-background p-1 shadow-md"
                    >
                      {models.map((model) => (
                        <button
                          key={model.id}
                          type="button"
                          role="option"
                          aria-selected={model.id === selectedModel}
                          className="flex w-full items-center justify-between gap-2 rounded px-2 py-1.5 text-left text-xs hover:bg-accent"
                          onClick={() => {
                            setModelOverride(model.id === defaultModel ? null : model.id)
                            setShowModels(false)
                          }}
                        >
                          <span className="truncate">{model.id}</span>
                        </button>
                      ))}
                    </div>
                  </>
                ) : null}
              </div>
            ) : null}
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
                onClick={() => void send()}
                disabled={isDisabled || isSending || hasUploading || (!hasText && !hasUploaded)}
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
