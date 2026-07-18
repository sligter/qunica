import { useRef, useState } from 'react'
import { FolderPlus } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useCreateWorkspace, useWorkspaces } from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api-v2/client'
import {
  composePickedPath,
  pickFolder,
  readRememberedPrefix,
  saveRememberedPrefix,
  type FolderPickResult,
} from '@/lib/folderPicker'
import { cn } from '@/lib/utils'
import type { WorkspaceBackendType } from '@/types/api'

interface WorkspaceFieldProps {
  value: string
  onChange: (workspaceId: string) => void
  error?: string
  variant?: 'default' | 'compact'
}

const PICKER_SCOPE = 'workspace-root'

function inferWorkspaceName(path: string) {
  const normalized = path.replace(/\\/g, '/').replace(/\/+$/, '')
  const last = normalized.split('/').filter(Boolean).pop()
  return last ?? ''
}

function localPathLooksAbsolute(path: string) {
  const trimmed = path.trim()
  return /^(?:[a-zA-Z]:[\\/]|\\\\|\/)/.test(trimmed)
}

export function WorkspaceField({
  value,
  onChange,
  error,
  variant = 'default',
}: WorkspaceFieldProps) {
  const { t } = useTranslation(['agents', 'common'])
  const workspaces = useWorkspaces()
  const createWorkspace = useCreateWorkspace()
  const fallbackInputRef = useRef<HTMLInputElement | null>(null)
  const pathInputRef = useRef<HTMLInputElement | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [workspaceName, setWorkspaceName] = useState('')
  const [localPath, setLocalPath] = useState('')
  const [createError, setCreateError] = useState<string | null>(null)

  const selected = (workspaces.data ?? []).find((workspace) => workspace.id === value)
  const selectedBackendType: WorkspaceBackendType = selected?.backend_type ?? 'local'
  const backendLabel = (backend: WorkspaceBackendType) =>
    backend === 'local' ? t('agents:states.backendLocal') : t('agents:states.backendSandbox')

  const onManualPathChange = (nextPath: string) => {
    setLocalPath(nextPath)
    if (!workspaceName) {
      setWorkspaceName(inferWorkspaceName(nextPath))
    }
    saveRememberedPrefix(PICKER_SCOPE, nextPath)
  }

  const applyPick = (folderName: string, absolutePath?: string) => {
    if (!folderName) return
    const remembered = readRememberedPrefix(PICKER_SCOPE)
    const composed = absolutePath ?? composePickedPath(localPath, folderName, remembered)
    setLocalPath(composed)
    saveRememberedPrefix(PICKER_SCOPE, composed)
    if (!workspaceName) {
      setWorkspaceName(folderName)
    }
    requestAnimationFrame(() => {
      pathInputRef.current?.focus()
    })
  }

  const onPickFolder = async () => {
    setCreateError(null)
    const result: FolderPickResult = await pickFolder()
    if (result.kind === 'native') {
      applyPick(result.name, result.path)
      return
    }
    if (result.kind === 'cancelled') {
      return
    }
    if (result.kind === 'error') {
      setCreateError(result.message)
      return
    }
    fallbackInputRef.current?.click()
  }

  const onFallbackChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    const relative = file?.webkitRelativePath
    if (relative) {
      const folderName = relative.split('/')[0] ?? ''
      applyPick(folderName)
    }
    if (fallbackInputRef.current) {
      fallbackInputRef.current.value = ''
    }
  }

  const onCreate = async () => {
    if (!localPathLooksAbsolute(localPath)) {
      setCreateError(
        t('agents:workspacePicker.absolutePath'),
      )
      return
    }
    setCreateError(null)
    try {
      const created = await createWorkspace.mutateAsync({
        name: workspaceName,
        backend_type: 'local',
        local_path: localPath,
      })
      onChange(created.id)
      setWorkspaceName('')
      setLocalPath('')
      setShowCreate(false)
    } catch (err) {
      setCreateError(err instanceof ApiError ? err.message : t('agents:errors.network'))
    }
  }

  return (
    <div className="space-y-2">
      <div className="space-y-1.5">
        <div className="flex items-center justify-between gap-2">
          {variant === 'default' ? <Label htmlFor="agent-workspace">{t('agents:fields.workspace')}</Label> : <span />}
          <Button type="button" variant="outline" size="sm" onClick={() => setShowCreate(!showCreate)}>
            {showCreate ? (
              t('common:actions.cancel')
            ) : variant === 'compact' ? (
              <>
                <FolderPlus className="h-3.5 w-3.5" />
                {t('agents:workspacePicker.new')}
              </>
            ) : (
              t('agents:workspacePicker.newLocal')
            )}
          </Button>
        </div>
        <select
          id="agent-workspace"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          className="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <option value="">{t('agents:workspacePicker.select')}</option>
          {(workspaces.data ?? []).map((workspace) => (
            <option key={workspace.id} value={workspace.id}>
              {workspace.name} — {backendLabel(workspace.backend_type)}
            </option>
          ))}
        </select>
        {error && <p className="text-xs text-destructive">{error}</p>}
        {workspaces.data && workspaces.data.length === 0 && !showCreate && (
          <p className="text-[11px] text-muted-foreground">
            {t('agents:workspacePicker.createFirst')}
          </p>
        )}
        {selected && (
          <p
            className="truncate text-[11px] text-muted-foreground"
            title={selected.local_path ?? selected.sandbox_ref ?? undefined}
          >
            {variant === 'compact'
              ? t('agents:workspacePicker.location', {
                  location: selected.local_path ?? selected.sandbox_ref ?? t('agents:workspacePicker.notConfigured'),
                })
              : t('agents:workspacePicker.bound', {
                  backend: backendLabel(selectedBackendType),
                  location: selected.local_path ?? selected.sandbox_ref ?? t('agents:workspacePicker.notConfigured'),
                })}
          </p>
        )}
      </div>

      {showCreate && (
        <div className="space-y-3 rounded-md border border-border bg-card p-3">
          <div className="space-y-1.5">
            <Label htmlFor="workspace-name">{t('agents:workspacePicker.name')}</Label>
            <Input
              id="workspace-name"
              value={workspaceName}
              onChange={(event) => setWorkspaceName(event.target.value)}
              placeholder={t('agents:workspacePicker.namePlaceholder')}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="workspace-path">{t('agents:workspacePicker.localPath')}</Label>
            <div className="flex gap-2">
              <Input
                id="workspace-path"
                ref={pathInputRef}
                value={localPath}
                onChange={(event) => onManualPathChange(event.target.value)}
                placeholder={t('agents:workspacePicker.pathPlaceholder')}
                className={cn(localPath && !localPathLooksAbsolute(localPath) && 'border-destructive')}
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => void onPickFolder()}
              >
                {t('agents:workspacePicker.pickFolder')}
              </Button>
            </div>
            <input
              ref={fallbackInputRef}
              type="file"
              className="hidden"
              multiple
              {...({
                webkitdirectory: '',
                directory: '',
              } as Record<string, string>)}
              onChange={onFallbackChange}
            />
          </div>
          {createError && <p className="text-xs text-destructive">{createError}</p>}
          <Button
            type="button"
            size="sm"
            disabled={createWorkspace.isPending || !workspaceName || !localPath}
            onClick={() => void onCreate()}
          >
            {createWorkspace.isPending ? t('agents:workspacePicker.creating') : t('agents:workspacePicker.create')}
          </Button>
        </div>
      )}
    </div>
  )
}
