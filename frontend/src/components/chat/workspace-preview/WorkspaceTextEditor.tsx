import { useEffect, useId, useMemo, useRef, useState } from 'react'
import hljs from 'highlight.js/lib/common'
import powershell from 'highlight.js/lib/languages/powershell'
import {
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  RefreshCw,
  Replace,
  ReplaceAll,
  Save,
  X,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Textarea } from '@/components/ui/textarea'
import {
  useSaveConversationWorkspaceFileText,
  type WorkspaceAgentScope,
} from '@/hooks/useConversationWorkspaceFiles'
import {
  workspaceErrorMessageKey,
  type WorkspaceErrorMessageKey,
} from '@/i18n/localizedError'
import { ApiError } from '@/lib/api-v2/client'
import { cn } from '@/lib/utils'
import type {
  ConversationScope,
  ConversationWorkspaceFileTextResponse,
} from '@/types/api'

interface WorkspaceTextEditorProps {
  scope: ConversationScope
  conversationId: string
  agentId?: WorkspaceAgentScope
  file: ConversationWorkspaceFileTextResponse
  presentation?: 'dialog' | 'editor'
  onDirtyChange?: (dirty: boolean) => void
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

hljs.registerLanguage('powershell', powershell)

const LANGUAGE_BY_EXTENSION: Record<string, string> = {
  c: 'c', cc: 'cpp', cpp: 'cpp', cs: 'csharp', css: 'css', go: 'go', h: 'c',
  hpp: 'cpp', html: 'xml', htm: 'xml', java: 'java', js: 'javascript', jsx: 'javascript',
  json: 'json', md: 'markdown', markdown: 'markdown', php: 'php', ps1: 'powershell',
  py: 'python', rb: 'ruby', rs: 'rust', sh: 'shell', sql: 'sql', svelte: 'xml',
  toml: 'ini', ts: 'typescript', tsx: 'typescript', vue: 'xml', xml: 'xml', yml: 'yaml', yaml: 'yaml',
}

function sourceLanguage(path: string): string | null {
  const name = path.replaceAll('\\', '/').split('/').at(-1)?.toLowerCase() ?? ''
  if (name === 'dockerfile') return 'shell'
  if (name === 'makefile') return hljs.getLanguage(name) ? name : null
  const extension = name.includes('.') ? name.split('.').at(-1) ?? '' : ''
  const language = LANGUAGE_BY_EXTENSION[extension]
  return language && hljs.getLanguage(language) ? language : null
}

function matchPositions(content: string, query: string): number[] {
  if (!query) return []
  const source = content.toLocaleLowerCase()
  const needle = query.toLocaleLowerCase()
  const positions: number[] = []
  let index = source.indexOf(needle)
  while (index >= 0) {
    positions.push(index)
    index = source.indexOf(needle, index + needle.length)
  }
  return positions
}

function revealTextareaPosition(
  editor: HTMLTextAreaElement,
  content: string,
  position: number,
  length: number,
) {
  const before = content.slice(0, position)
  const line = before.split('\n').length - 1
  const column = position - (before.lastIndexOf('\n') + 1)
  const style = getComputedStyle(editor)
  const fontSize = Number.parseFloat(style.fontSize) || 12
  const lineHeight = Number.parseFloat(style.lineHeight) || 20
  const paddingTop = Number.parseFloat(style.paddingTop) || 0
  const paddingLeft = Number.parseFloat(style.paddingLeft) || 0
  const top = paddingTop + line * lineHeight
  editor.scrollTop = Math.max(0, top - editor.clientHeight / 2 + lineHeight / 2)

  const characterWidth = fontSize * 0.62
  const left = paddingLeft + column * characterWidth
  const right = left + length * characterWidth
  if (left < editor.scrollLeft || right > editor.scrollLeft + editor.clientWidth) {
    editor.scrollLeft = Math.max(0, left - editor.clientWidth / 3)
  }
}

export function WorkspaceTextEditor({
  scope,
  conversationId,
  agentId = null,
  file,
  presentation = 'dialog',
  onDirtyChange,
  onRefresh,
}: WorkspaceTextEditorProps) {
  const { t } = useTranslation('chat')
  const save = useSaveConversationWorkspaceFileText(scope, conversationId, agentId)
  const [snapshot, setSnapshot] = useState<EditorSnapshot>(() => snapshotFromFile(file))
  const [draft, setDraft] = useState(() => file.content ?? '')
  const [conflict, setConflict] = useState(false)
  const [refreshing, setRefreshing] = useState(false)
  const [refreshError, setRefreshError] = useState<WorkspaceErrorMessageKey | null>(null)
  const [confirmRefreshOpen, setConfirmRefreshOpen] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [replaceOpen, setReplaceOpen] = useState(false)
  const [searchValue, setSearchValue] = useState('')
  const [replaceValue, setReplaceValue] = useState('')
  const [matchIndex, setMatchIndex] = useState(-1)
  const observedFile = useRef({ path: file.path, version: file.version })
  const currentPath = useRef(file.path)
  const draftRevision = useRef(0)
  const editorRef = useRef<HTMLTextAreaElement>(null)
  const highlightRef = useRef<HTMLPreElement>(null)
  const searchInputRef = useRef<HTMLInputElement>(null)
  const editorId = useId()
  currentPath.current = file.path
  const dirty = draft !== snapshot.content
  const matches = matchPositions(draft, searchValue)
  const language = sourceLanguage(snapshot.path)
  const highlighted = useMemo(
    () => language ? hljs.highlight(draft, { language, ignoreIllegals: true }).value : null,
    [draft, language],
  )

  useEffect(() => onDirtyChange?.(dirty), [dirty, onDirtyChange])

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
      const errorKey = workspaceErrorMessageKey(error)
      const message = t('workspace.previewPanel.refreshError', {
        message: t(errorKey),
      })
      setRefreshError(errorKey)
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

  const updateDraft = (content: string) => {
    draftRevision.current += 1
    setDraft(content)
  }

  const openSearch = (withReplace: boolean) => {
    const editor = editorRef.current
    const selection = editor?.value.slice(editor.selectionStart, editor.selectionEnd) ?? ''
    if (selection && !selection.includes('\n')) setSearchValue(selection)
    setSearchOpen(true)
    setReplaceOpen(withReplace)
    requestAnimationFrame(() => {
      searchInputRef.current?.focus()
      searchInputRef.current?.select()
    })
  }

  const findMatch = (
    direction: 1 | -1,
    query = searchValue,
    restart = false,
    focusEditor = true,
  ) => {
    const editor = editorRef.current
    const positions = matchPositions(draft, query)
    if (!editor || positions.length === 0) {
      setMatchIndex(-1)
      return
    }
    const cursor = restart
      ? (direction === 1 ? 0 : draft.length)
      : (direction === 1 ? editor.selectionEnd : editor.selectionStart)
    const position = direction === 1
      ? (positions.find((candidate) => candidate >= cursor) ?? positions[0]!)
      : (positions.filter((candidate) => candidate < cursor).at(-1) ?? positions.at(-1)!)
    if (focusEditor) editor.focus()
    editor.setSelectionRange(position, position + query.length)
    revealTextareaPosition(editor, draft, position, query.length)
    setMatchIndex(positions.indexOf(position))
  }

  const replaceCurrent = () => {
    const editor = editorRef.current
    if (!editor || !searchValue) return
    const { selectionStart: start, selectionEnd: end } = editor
    if (draft.slice(start, end).toLocaleLowerCase() !== searchValue.toLocaleLowerCase()) {
      findMatch(1)
      return
    }
    const next = `${draft.slice(0, start)}${replaceValue}${draft.slice(end)}`
    updateDraft(next)
    requestAnimationFrame(() => {
      editor.focus()
      editor.setSelectionRange(start, start + replaceValue.length)
    })
  }

  const replaceEveryMatch = () => {
    const positions = matchPositions(draft, searchValue)
    if (positions.length === 0) return
    let cursor = 0
    let next = ''
    for (const position of positions) {
      next += draft.slice(cursor, position) + replaceValue
      cursor = position + searchValue.length
    }
    updateDraft(next + draft.slice(cursor))
    setMatchIndex(-1)
    requestAnimationFrame(() => editorRef.current?.focus())
  }

  const handleEditorKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.nativeEvent.isComposing || event.keyCode === 229) return
    const modifier = event.ctrlKey || event.metaKey
    if (!modifier || event.altKey) {
      if (event.key === 'Escape' && searchOpen) setSearchOpen(false)
      return
    }
    switch (event.key.toLocaleLowerCase()) {
      case 'f':
        event.preventDefault()
        openSearch(false)
        break
      case 'h':
        event.preventDefault()
        openSearch(true)
        break
      case 's':
        event.preventDefault()
        void handleSave()
        break
    }
  }

  const saveError = save.error
    ? t('workspace.previewPanel.saveError', {
        message: t(workspaceErrorMessageKey(save.error)),
      })
    : null

  return (
    <div
      className={cn(
        'flex flex-col',
        presentation === 'editor' ? 'h-full min-h-[28rem]' : 'min-h-[22rem]',
      )}
      data-preview-kind="text"
    >
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border pb-3">
        <span
          className={cn(
            'rounded-full border px-2 py-1 text-[10px] font-medium uppercase tracking-wide',
            dirty
              ? 'border-warning bg-warning text-warning-foreground'
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

      {searchOpen ? (
        <div className="mt-3 ml-auto w-full max-w-lg rounded-md border border-border bg-muted/35 p-1.5 shadow-sm">
          <div className="flex items-center gap-1">
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-7 w-7 shrink-0"
              onClick={() => setReplaceOpen((open) => !open)}
              aria-label={t('workspace.previewPanel.toggleReplace')}
              aria-expanded={replaceOpen}
            >
              {replaceOpen
                ? <ChevronDown className="h-3.5 w-3.5" />
                : <ChevronRight className="h-3.5 w-3.5" />}
            </Button>
            <input
              ref={searchInputRef}
              type="search"
              value={searchValue}
              aria-label={t('workspace.previewPanel.find')}
              placeholder={t('workspace.previewPanel.findPlaceholder')}
              className="h-7 min-w-0 flex-1 rounded-sm border border-input bg-background px-2 font-mono text-xs outline-none focus:ring-1 focus:ring-ring"
              onChange={(event) => {
                const query = event.target.value
                setSearchValue(query)
                requestAnimationFrame(() => findMatch(1, query, true, false))
              }}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault()
                  findMatch(event.shiftKey ? -1 : 1)
                } else if (event.key === 'Escape') {
                  setSearchOpen(false)
                  editorRef.current?.focus()
                }
              }}
            />
            <span className="min-w-14 text-center text-[10px] tabular-nums text-muted-foreground" aria-live="polite">
              {matches.length > 0
                ? `${Math.max(1, matchIndex + 1)}/${matches.length}`
                : t('workspace.previewPanel.noMatches')}
            </span>
            <Button type="button" variant="ghost" size="icon" className="h-7 w-7" onClick={() => findMatch(-1)} aria-label={t('workspace.previewPanel.previousMatch')}>
              <ChevronUp className="h-3.5 w-3.5" />
            </Button>
            <Button type="button" variant="ghost" size="icon" className="h-7 w-7" onClick={() => findMatch(1)} aria-label={t('workspace.previewPanel.nextMatch')}>
              <ChevronDown className="h-3.5 w-3.5" />
            </Button>
            <Button type="button" variant="ghost" size="icon" className="h-7 w-7" onClick={() => setSearchOpen(false)} aria-label={t('workspace.previewPanel.closeFind')}>
              <X className="h-3.5 w-3.5" />
            </Button>
          </div>
          {replaceOpen ? (
            <div className="mt-1 flex items-center gap-1 pl-8">
              <input
                value={replaceValue}
                aria-label={t('workspace.previewPanel.replace')}
                placeholder={t('workspace.previewPanel.replacePlaceholder')}
                className="h-7 min-w-0 flex-1 rounded-sm border border-input bg-background px-2 font-mono text-xs outline-none focus:ring-1 focus:ring-ring"
                onChange={(event) => setReplaceValue(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') {
                    event.preventDefault()
                    replaceCurrent()
                  }
                }}
              />
              <Button type="button" variant="ghost" size="icon" className="h-7 w-7" onClick={replaceCurrent} aria-label={t('workspace.previewPanel.replaceCurrent')}>
                <Replace className="h-3.5 w-3.5" />
              </Button>
              <Button type="button" variant="ghost" size="icon" className="h-7 w-7" onClick={replaceEveryMatch} aria-label={t('workspace.previewPanel.replaceAll')}>
                <ReplaceAll className="h-3.5 w-3.5" />
              </Button>
            </div>
          ) : null}
        </div>
      ) : null}

      {snapshot.truncated ? (
        <div className="mt-3 flex gap-2 rounded-md border border-warning bg-warning p-3 text-xs leading-5 text-warning-foreground">
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
          {t('workspace.previewPanel.refreshError', { message: t(refreshError) })}
        </p>
      ) : null}

      <label htmlFor={editorId} className="sr-only">
        {t('workspace.previewPanel.editorLabel', { name: file.name })}
      </label>
      {language && highlighted !== null ? (
        <div
          className={cn(
            'workspace-code-editor relative mt-3 flex-1 overflow-hidden rounded-md border border-input bg-background shadow-sm',
            presentation === 'editor' ? 'min-h-[24rem]' : 'min-h-[20rem]',
          )}
        >
          <pre
            ref={highlightRef}
            aria-hidden="true"
            data-language={language}
            className="pointer-events-none absolute inset-0 m-0 overflow-hidden whitespace-pre px-3 py-2 font-mono text-xs leading-5"
          >
            <code dangerouslySetInnerHTML={{ __html: highlighted }} />
          </pre>
          <Textarea
            ref={editorRef}
            id={editorId}
            value={draft}
            readOnly={snapshot.truncated || save.isPending || refreshing}
            aria-readonly={snapshot.truncated || save.isPending || refreshing}
            aria-busy={save.isPending || refreshing}
            onChange={(event) => updateDraft(event.target.value)}
            onKeyDown={handleEditorKeyDown}
            onScroll={(event) => {
              if (!highlightRef.current) return
              highlightRef.current.scrollTop = event.currentTarget.scrollTop
              highlightRef.current.scrollLeft = event.currentTarget.scrollLeft
            }}
            spellCheck={false}
            wrap="off"
            className="workspace-code-input relative z-10 h-full min-h-full resize-none whitespace-pre border-0 bg-transparent font-mono text-xs leading-5 shadow-none focus-visible:ring-0"
          />
        </div>
      ) : (
        <Textarea
          ref={editorRef}
          id={editorId}
          value={draft}
          readOnly={snapshot.truncated || save.isPending || refreshing}
          aria-readonly={snapshot.truncated || save.isPending || refreshing}
          aria-busy={save.isPending || refreshing}
          onChange={(event) => updateDraft(event.target.value)}
          onKeyDown={handleEditorKeyDown}
          spellCheck={false}
          className={cn(
            'mt-3 flex-1 whitespace-pre font-mono text-xs leading-5',
            presentation === 'editor' ? 'min-h-[24rem] resize-none' : 'min-h-[20rem] resize-y',
          )}
        />
      )}

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
