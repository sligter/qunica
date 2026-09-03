import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ServerFolderPicker } from '@/components/workspace/ServerFolderPicker'
import { useCreateWorkspace } from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api-v2/client'
import {
  basename,
  pickFolder,
  saveRememberedPrefix,
} from '@/lib/folderPicker'
import {
  localizedErrorText,
  messageError,
  translatedError,
  type LocalizedError,
} from '@/i18n/localizedError'

const PICKER_SCOPE = 'workspace-management-root'

export function WorkspaceCreatePage() {
  const { t } = useTranslation('workspaces')
  const navigate = useNavigate()
  const createWorkspace = useCreateWorkspace()
  const [name, setName] = useState('')
  const [localPath, setLocalPath] = useState('')
  const [browsing, setBrowsing] = useState(false)
  const [error, setError] = useState<LocalizedError | null>(null)

  const trimmedName = name.trim()
  const trimmedPath = localPath.trim()
  const canCreate =
    trimmedName.length > 0 &&
    trimmedPath.length > 0 &&
    !createWorkspace.isPending

  const onPathChange = (nextPath: string) => {
    setLocalPath(nextPath)
    saveRememberedPrefix(PICKER_SCOPE, nextPath)
    if (!name.trim()) {
      setName(basename(nextPath.trim()) || '')
    }
  }

  const applyPickedPath = (nextPath: string) => {
    setLocalPath(nextPath)
    saveRememberedPrefix(PICKER_SCOPE, nextPath)
    if (!name.trim()) {
      setName(basename(nextPath) || nextPath)
    }
  }

  const onPickFolder = async () => {
    setError(null)
    const result = await pickFolder()
    if (result.kind === 'native') {
      applyPickedPath(result.path ?? result.name)
      return
    }
    if (result.kind === 'cancelled') return
    if (result.kind === 'serverBrowse') {
      setBrowsing(true)
      return
    }
    setError(messageError(result.message))
  }

  const onSubmit = (e: React.FormEvent) => {
    e.preventDefault()
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
          setError(
            err instanceof ApiError
              ? messageError(err.message)
              : translatedError('errors.create'),
          )
        },
      },
    )
  }

  return (
    <DetailShell
      title={t('form.createTitle')}
      subtitle={t('form.createSubtitle')}
    >
      <form onSubmit={onSubmit} className="space-y-6">
        <div className="rounded-xl border border-border/80 bg-card p-6 shadow-xs space-y-5">
          <div className="space-y-2">
            <Label htmlFor="workspace-new-name" className="text-sm font-medium">
              {t('fields.name')}
            </Label>
            <Input
              id="workspace-new-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t('form.namePlaceholder')}
              className="w-full"
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="workspace-new-path" className="text-sm font-medium">
              {t('fields.backendLocalPath')}
            </Label>
            <div className="flex gap-2">
              <Input
                id="workspace-new-path"
                value={localPath}
                onChange={(event) => onPathChange(event.target.value)}
                placeholder={t('validation.pathPlaceholder')}
              />
              <Button type="button" variant="outline" onClick={() => void onPickFolder()}>
                {t('actions.pickFolder')}
              </Button>
              {browsing ? (
                <ServerFolderPicker
                  open
                  onOpenChange={setBrowsing}
                  onSelect={(absolutePath) => applyPickedPath(absolutePath)}
                />
              ) : null}
            </div>
            <p className="text-xs text-muted-foreground">
              {t('fields.backendLocalPathDescription')}
            </p>
          </div>

          {localizedErrorText(error, t) ? (
            <div className="rounded-lg bg-destructive/10 p-3 text-xs text-destructive" role="alert">
              {localizedErrorText(error, t)}
            </div>
          ) : null}
        </div>

        <div className="flex items-center gap-3">
          <Button type="submit" disabled={!canCreate}>
            {createWorkspace.isPending ? t('actions.creating') : t('actions.create')}
          </Button>
          <Button type="button" variant="ghost" onClick={() => void navigate('/workspaces')}>
            {t('common:actions.cancel', '取消')}
          </Button>
        </div>
      </form>
    </DetailShell>
  )
}
