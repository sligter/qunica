import { useEffect, useMemo, useState } from 'react'
import {
  Binary,
  FileCode2,
  FileText,
  FolderOpen,
  Search,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { FieldError } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { PageState } from '@/components/ui/page-state'
import { Section } from '@/components/ui/section'
import { Skeleton } from '@/components/ui/skeleton'
import { Textarea } from '@/components/ui/textarea'
import {
  useSkillResource,
  useSkillResources,
  useUpdateSkillResource,
} from '@/hooks/useSkills'
import {
  useUnsavedChangesAction,
  useUnsavedChangesGuard,
} from '@/hooks/useUnsavedChangesGuard'
import { ApiError } from '@/lib/api-v2/client'
import { navItemClass } from '@/lib/navItemClass'
import type { SkillFileInfo, SkillRead } from '@/types/api'
import {
  localizedErrorText,
  messageError,
  translatedError,
  type LocalizedError,
} from '@/i18n/localizedError'

interface SkillResourcesPanelProps {
  skill: SkillRead
}

export function SkillResourcesPanel({ skill }: SkillResourcesPanelProps) {
  const { t } = useTranslation('skills')
  const resources = useSkillResources(skill.id)
  const requestAction = useUnsavedChangesAction()
  // Metadata comes with the skill; the resources query only adds editability
  // and content. Merging keeps the browser responsive before that fetch
  // resolves instead of flashing an empty list.
  const rows: SkillFileInfo[] = useMemo(
    () => resources.data ?? skill.files ?? [],
    [resources.data, skill.files],
  )
  const [selectedPath, setSelectedPath] = useState<string | null>(null)
  const [query, setQuery] = useState('')

  // Default to the first file once rows exist, without pinning state to a
  // value that changes between renders.
  useEffect(() => {
    setSelectedPath((current) =>
      current && rows.some((row) => row.path === current) ? current : rows[0]?.path ?? null,
    )
  }, [rows])

  const selectedMeta = rows.find((row) => row.path === selectedPath) ?? null

  const visibleRows = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return rows
    return rows.filter((file) => file.path.toLowerCase().includes(q))
  }, [query, rows])

  if (!skill.files || skill.files.length === 0) {
    return (
      <Section title={t('resources.title')} as="h3">
        <p className="text-sm text-muted-foreground">{t('resources.empty')}</p>
      </Section>
    )
  }

  return (
    <Section
      title={t('resources.title')}
      as="h3"
      aside={
        <Badge variant="outline" className="text-2xs font-medium">
          {t('resources.file', { count: skill.files.length })}
        </Badge>
      }
      contentClassName="space-y-3"
    >
      {resources.error ? <FieldError>{t('resources.metadataError')}</FieldError> : null}

      <div className="grid min-h-80 gap-3 lg:grid-cols-[minmax(0,17rem),minmax(0,1fr)]">
        <div className="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-lg border border-border bg-card shadow-sm">
          <div className="shrink-0 border-b border-border p-2">
            <div className="relative">
              <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder={t('resources.search')}
                className="h-8 border-none bg-muted pl-8 text-xs focus-visible:bg-background"
                aria-label={t('resources.search')}
              />
            </div>
          </div>
          <div className="min-h-0 flex-1 space-y-0.5 overflow-y-auto p-1">
            {visibleRows.map((file) => {
              const active = file.path === selectedPath
              return (
                <button
                  key={file.path}
                  type="button"
                  aria-current={active ? 'true' : undefined}
                  title={file.path}
                  onClick={() => requestAction(() => setSelectedPath(file.path))}
                  className={navItemClass(active, 'items-center gap-2 px-2 py-1.5')}
                >
                  <ResourceFileIcon category={file.category} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-xs leading-5">
                      {resourceFileName(file.path)}
                    </span>
                    <span className="block truncate text-2xs font-normal leading-4 text-muted-foreground">
                      {resourceDirName(file.path)}
                    </span>
                  </span>
                </button>
              )
            })}
            {visibleRows.length === 0 && (
              <p className="px-2 py-6 text-center text-xs text-muted-foreground">
                {t('resources.noMatches')}
              </p>
            )}
          </div>
        </div>

        <ResourceViewer
          skillId={skill.id}
          path={selectedPath}
          meta={selectedMeta}
          metadataLoading={resources.isLoading}
        />
      </div>
    </Section>
  )
}

interface ResourceViewerProps {
  skillId: string
  path: string | null
  /** File metadata for the viewer header while content loads (or fails). */
  meta: (SkillFileInfo & { is_text?: boolean }) | null
  metadataLoading: boolean
}

function ResourceViewer({ skillId, path, meta, metadataLoading }: ResourceViewerProps) {
  const { t } = useTranslation('skills')
  // The list endpoint already marks missing and binary resources as non-text.
  // Do not turn that known result into a retried per-file request.
  const resource = useSkillResource(skillId, metadataLoading || meta?.is_text === false ? null : path)
  const update = useUpdateSkillResource(skillId, path)
  const [draft, setDraft] = useState('')
  const [saveError, setSaveError] = useState<LocalizedError | null>(null)
  const savedContent = resource.data?.content ?? ''

  const canEdit =
    !resource.isLoading &&
    !resource.error &&
    resource.data?.is_text === true &&
    typeof resource.data.content === 'string'
  const dirty = canEdit && draft !== savedContent
  useUnsavedChangesGuard(dirty)

  // The draft follows whichever file is selected; resetting on change is what
  // stops one file's unsaved edits leaking into another.
  useEffect(() => {
    setDraft(resource.data?.content ?? '')
    setSaveError(null)
  }, [path, resource.data])

  if (!path || !meta) {
    return (
      <div className="grid flex-1 place-items-center rounded-lg border border-dashed border-border bg-card/50 text-sm text-muted-foreground">
        <PageState inset title={t('resources.select')} icon={FolderOpen} />
      </div>
    )
  }

  const loadErrorMessage =
    resource.error instanceof ApiError
      ? t('resources.loadErrorDetail', { message: resource.error.message })
      : resource.error
        ? t('resources.loadError')
        : null
  const saveErrorText = localizedErrorText(saveError, t)

  return (
    <div className="flex min-h-72 min-w-0 flex-col overflow-hidden rounded-lg border border-border bg-card shadow-sm">
      {/* Only the name is bold here; the directory stays in the list row, so a
          deep path can't crowd out the actions on narrow panes. */}
      <div className="flex shrink-0 items-center justify-between gap-3 border-b border-border px-3 py-2">
        <div className="flex min-w-0 items-center gap-2">
          <ResourceFileIcon category={meta.category} />
          <p className="truncate text-sm font-medium">{resourceFileName(meta.path)}</p>
        </div>
        <Badge variant="outline" className="shrink-0 text-2xs font-medium">
          {meta.category}
        </Badge>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {metadataLoading || resource.isLoading ? (
          // Lines rather than a sentence: the editor that arrives is a block of
          // monospace text, so a skeleton in that shape is what keeps the pane
          // from resizing when the content lands.
          <div className="space-y-2 p-4" aria-hidden>
            <span className="sr-only">{t('resources.loading')}</span>
            <Skeleton className="h-3 w-4/5" />
            <Skeleton className="h-3 w-2/3 opacity-70" />
            <Skeleton className="h-3 w-11/12 opacity-70" />
            <Skeleton className="h-3 w-1/2 opacity-60" />
            <Skeleton className="h-3 w-3/4 opacity-60" />
            <Skeleton className="h-3 w-2/5 opacity-50" />
          </div>
        ) : loadErrorMessage ? (
          <PageState
            variant="error"
            title={loadErrorMessage}
            description={t('resources.loadError')}
          />
        ) : canEdit ? (
          <Textarea
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            spellCheck={false}
            aria-label={t('resources.editable')}
            // `ring-inset` on focus: the editor has no border of its own, so
            // dropping the ring entirely (as it did) left keyboard focus with
            // no visible sign at all.
            className="h-full min-h-64 resize-none overflow-auto rounded-none border-none bg-code font-mono text-xs leading-relaxed text-code-foreground shadow-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
          />
        ) : (
          <PageState
            inset
            title={t('resources.binaryHint')}
            icon={Binary}
            description={t('resources.notEditable')}
          />
        )}
      </div>

      {(canEdit || saveErrorText) && (
        // The error takes its own row. Sharing one line with the file size and
        // the Save button meant a message longer than about twelve characters
        // was truncated to an ellipsis — the least useful part of a failure is
        // the part you cannot read.
        <div className="shrink-0 border-t border-border bg-background/60 px-3 py-2">
          {saveErrorText ? <FieldError className="mb-2">{saveErrorText}</FieldError> : null}
          <div className="flex items-center justify-between gap-3">
            <p className="min-w-0 truncate text-2xs text-muted-foreground">
              {formatSize(meta.size)}
              {' · '}
              {canEdit ? t('resources.text') : t('resources.notEditable')}
            </p>
            <Button
              size="sm"
              className="shrink-0"
              disabled={update.isPending || !dirty}
              onClick={() => {
                setSaveError(null)
                update.mutate(draft, {
                  onError: (err) => {
                    setSaveError(
                      err instanceof ApiError
                        ? messageError(err.message, 'resources.saveErrorDetail')
                        : translatedError('resources.saveError'),
                    )
                  },
                })
              }}
            >
              {update.isPending ? t('resources.saving') : t('resources.save')}
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}

const RESOURCE_ICON_CLASS = 'h-3.5 w-3.5 shrink-0 text-primary'

function ResourceFileIcon({ category }: { category: string }) {
  if (/script|tool|code/i.test(category)) {
    return <FileCode2 className={RESOURCE_ICON_CLASS} />
  }
  return <FileText className={RESOURCE_ICON_CLASS} />
}

function resourceFileName(path: string) {
  return path.split('/').pop() ?? path
}

function resourceDirName(path: string) {
  const parts = path.split('/')
  parts.pop()
  return parts.join('/') || '/'
}

function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
