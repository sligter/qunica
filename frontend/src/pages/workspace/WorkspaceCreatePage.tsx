import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useCreateWorkspace } from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api-v2/client'
import {
  basename,
  composePickedPath,
  looksAbsolute,
  pickFolder,
  readRememberedPrefix,
  saveRememberedPrefix,
} from '@/lib/folderPicker'

const PICKER_SCOPE = 'workspace-management-root'

export function WorkspaceCreatePage() {
  const { t } = useTranslation('workspaces')
  const navigate = useNavigate()
  const createWorkspace = useCreateWorkspace()
  const [name, setName] = useState('')
  const [localPath, setLocalPath] = useState('')
  const [error, setError] = useState<string | null>(null)

  const trimmedName = name.trim()
  const trimmedPath = localPath.trim()
  const canCreate =
    trimmedName.length > 0 &&
    trimmedPath.length > 0 &&
    looksAbsolute(trimmedPath) &&
    !createWorkspace.isPending

  const onPathChange = (nextPath: string) => {
    setLocalPath(nextPath)
    saveRememberedPrefix(PICKER_SCOPE, nextPath)
    if (!name.trim()) {
      setName(basename(nextPath.trim()) || '')
    }
  }

  const onPickFolder = async () => {
    setError(null)
    const result = await pickFolder()
    if (result.kind === 'native') {
      const nextPath =
        result.path ??
        composePickedPath(localPath, result.name, readRememberedPrefix(PICKER_SCOPE))
      setLocalPath(nextPath)
      saveRememberedPrefix(PICKER_SCOPE, nextPath)
      if (!name.trim()) {
        setName(result.name)
      }
      return
    }
    if (result.kind === 'cancelled') return
    if (result.kind === 'fallback') {
      setError(t('validation.pickerUnavailable'))
      return
    }
    setError(result.message)
  }

  const onSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!looksAbsolute(trimmedPath)) {
      setError(t('validation.enterAbsolutePath'))
      return
    }
    setError(null)
    createWorkspace.mutate(
      {
        name: trimmedName,
        backend_type: 'local',
        local_path: trimmedPath,
      },
      {
        onSuccess: (created) => {
          void navigate(`/workspaces/${created.id}`)
        },
        onError: (err) => {
          setError(err instanceof ApiError ? err.message : t('errors.create'))
        },
      },
    )
  }

  return (
    <DetailShell
      title={t('form.createTitle')}
      subtitle={t('form.createSubtitle')}
    >
      <form onSubmit={onSubmit} className="space-y-4">
        <div className="space-y-1.5">
          <Label htmlFor="workspace-new-name">{t('fields.name')}</Label>
          <Input
            id="workspace-new-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={t('form.namePlaceholder')}
            className="max-w-xl"
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="workspace-new-path">{t('fields.backendLocalPath')}</Label>
          <div className="flex max-w-xl gap-2">
            <Input
              id="workspace-new-path"
              value={localPath}
              onChange={(event) => onPathChange(event.target.value)}
              placeholder={t('validation.pathPlaceholder')}
              className={
                trimmedPath && !looksAbsolute(trimmedPath)
                  ? 'border-destructive'
                  : undefined
              }
            />
            <Button type="button" variant="outline" onClick={() => void onPickFolder()}>
              {t('actions.pickFolder')}
            </Button>
          </div>
          {trimmedPath && !looksAbsolute(trimmedPath) ? (
            <p className="text-xs text-destructive">
              {t('validation.absolutePath')}
            </p>
          ) : null}
        </div>
        {error ? (
          <p className="text-sm text-destructive" role="alert">
            {error}
          </p>
        ) : null}
        <Button type="submit" disabled={!canCreate}>
          {createWorkspace.isPending ? t('actions.creating') : t('actions.create')}
        </Button>
      </form>
    </DetailShell>
  )
}
