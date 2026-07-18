import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import {
  useSkillResource,
  useSkillResources,
  useUpdateSkillResource,
} from '@/hooks/useSkills'
import { ApiError } from '@/lib/api-v2/client'
import { cn } from '@/lib/utils'
import type { SkillRead } from '@/types/api'

interface SkillResourcesPanelProps {
  skill: SkillRead
}

export function SkillResourcesPanel({ skill }: SkillResourcesPanelProps) {
  const { t } = useTranslation('skills')
  const resources = useSkillResources(skill.id)
  const [selectedPath, setSelectedPath] = useState<string | null>(null)
  const selected = useSkillResource(skill.id, selectedPath)
  const update = useUpdateSkillResource(skill.id, selectedPath)
  const [draft, setDraft] = useState('')
  const [saveError, setSaveError] = useState<string | null>(null)

  const resourceRows = resources.data ?? skill.files?.map((file) => ({
    ...file,
    is_text: isLikelyTextResource(file.path, file.size),
    content: null,
  })) ?? []
  const firstResourcePath = resourceRows[0]?.path

  useEffect(() => {
    if (!selectedPath && firstResourcePath) {
      setSelectedPath(firstResourcePath)
    }
  }, [firstResourcePath, selectedPath])

  useEffect(() => {
    if (selected.data?.content !== null && selected.data?.content !== undefined) {
      setDraft(selected.data.content)
      setSaveError(null)
    } else {
      setDraft('')
    }
  }, [selected.data])

  const selectedInfo = selected.data ?? resourceRows.find((row) => row.path === selectedPath)
  const canEditSelected =
    !selected.isLoading &&
    !selected.error &&
    selected.data?.is_text === true &&
    selected.data.content !== null &&
    selected.data.content !== undefined

  const onSave = async () => {
    setSaveError(null)
    try {
      await update.mutateAsync(draft)
    } catch (err) {
      setSaveError(err instanceof ApiError ? err.message : t('resources.saveError'))
    }
  }

  if (!skill.files || skill.files.length === 0) {
    return (
      <section className="space-y-2">
        <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          {t('resources.title')}
        </h3>
        <p className="text-sm text-muted-foreground">
          {t('resources.empty')}
        </p>
      </section>
    )
  }

  return (
    <section className="space-y-3">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          {t('resources.title')}
        </h3>
        <Badge variant="outline" className="text-[10px]">
          {t('resources.file', { count: skill.files.length })}
        </Badge>
      </div>

      {resources.error && (
        <p className="text-xs text-destructive">
          {t('resources.metadataError')}
        </p>
      )}

      <div className="grid min-h-72 gap-3 md:grid-cols-[minmax(0,240px),minmax(0,1fr)]">
        <div className="overflow-hidden rounded-md border border-border">
          <div className="max-h-80 overflow-y-auto p-1">
            {resourceRows.map((file) => (
              <button
                key={file.path}
                type="button"
                onClick={() => setSelectedPath(file.path)}
                className={cn(
                  'flex w-full flex-col items-start gap-1 rounded-sm px-2 py-1.5 text-left text-xs transition-colors',
                  file.path === selectedPath ? 'bg-primary/10' : 'hover:bg-card-hover',
                )}
              >
                <span className="w-full truncate font-medium">{file.path}</span>
                <span className="flex items-center gap-1 text-[10px] text-muted-foreground">
                  <Badge variant="outline" className="px-1 py-0 text-[9px]">
                    {file.category}
                  </Badge>
                  <span>{formatSize(file.size)}</span>
                  <span>{file.is_text ? t('resources.text') : t('resources.binaryUnknown')}</span>
                </span>
              </button>
            ))}
          </div>
        </div>

        <div className="min-w-0 rounded-md border border-border p-3">
          {!selectedInfo && (
            <p className="text-sm text-muted-foreground">{t('resources.select')}</p>
          )}
          {selectedInfo && (
            <div className="space-y-3">
              <div className="space-y-1">
                <p className="break-all text-sm font-medium">{selectedInfo.path}</p>
                <div className="flex flex-wrap gap-1.5">
                  <Badge variant="outline" className="text-[10px]">
                    {selectedInfo.category}
                  </Badge>
                  <Badge variant="secondary" className="text-[10px]">
                    {formatSize(selectedInfo.size)}
                  </Badge>
                  <Badge variant={selectedInfo.is_text ? 'default' : 'secondary'} className="text-[10px]">
                    {selectedInfo.is_text ? t('resources.editable') : t('resources.notEditable')}
                  </Badge>
                </div>
              </div>

              <Separator />

              {selected.isLoading && (
                <p className="text-sm text-muted-foreground">{t('resources.loading')}</p>
              )}
              {selected.error && (
                <p className="text-sm text-destructive">
                  {selected.error instanceof ApiError
                    ? selected.error.message
                    : t('resources.loadError')}
                </p>
              )}
              {canEditSelected && (
                  <div className="space-y-2">
                    <textarea
                      value={draft}
                      onChange={(event) => setDraft(event.target.value)}
                      className="h-64 w-full resize-y rounded-md border border-input bg-background p-3 font-mono text-xs outline-none focus:ring-1 focus:ring-ring"
                      spellCheck={false}
                    />
                    {saveError && (
                      <p className="text-xs text-destructive" role="alert">
                        {saveError}
                      </p>
                    )}
                    <Button size="sm" onClick={onSave} disabled={update.isPending || !canEditSelected}>
                      {update.isPending ? t('resources.saving') : t('resources.save')}
                    </Button>
                  </div>
                )}
              {!selected.isLoading && !selectedInfo.is_text && (
                <p className="text-sm text-muted-foreground">
                  {t('resources.binaryHint')}
                </p>
              )}
            </div>
          )}
        </div>
      </div>
    </section>
  )
}

const TEXT_RESOURCE_EXTENSIONS = new Set([
  '.md',
  '.markdown',
  '.txt',
  '.json',
  '.yaml',
  '.yml',
  '.toml',
  '.xml',
  '.html',
  '.css',
  '.js',
  '.jsx',
  '.ts',
  '.tsx',
  '.py',
  '.sh',
  '.bash',
  '.zsh',
  '.fish',
  '.ps1',
  '.sql',
  '.csv',
  '.ini',
  '.cfg',
])

const MAX_TEXT_RESOURCE_BYTES = 1_000_000

function isLikelyTextResource(path: string, size: number) {
  if (size > MAX_TEXT_RESOURCE_BYTES) return false
  const fileName = path.split('/').pop() ?? path
  const dotIndex = fileName.lastIndexOf('.')
  if (dotIndex < 0) return false
  return TEXT_RESOURCE_EXTENSIONS.has(fileName.slice(dotIndex).toLowerCase())
}

function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
