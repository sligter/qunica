import { useEffect, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { Bot, Users } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { DetailShell } from '@/components/layout/DetailShell'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import { PageState } from '@/components/ui/page-state'
import { DetailSkeleton } from '@/components/ui/skeleton'
import { SettingsRow, SettingsSection } from '@/components/ui/settings-row'
import { useAgents } from '@/hooks/useAgents'
import { useGroups, useUpdateGroup } from '@/hooks/useGroups'
import {
  useDeleteWorkspace,
  useUpdateWorkspace,
  useWorkspaces,
} from '@/hooks/useWorkspaces'
import { ApiError } from '@/lib/api-v2/client'
import {
  composePickedPath,
  looksAbsolute,
  pickFolder,
  readRememberedPrefix,
  saveRememberedPrefix,
} from '@/lib/folderPicker'
import type { GroupRead, WorkspaceRead, WorkspaceUpdate } from '@/types/api'
import { formatResourceStatus } from '@/i18n/resourceStatus'
import { localizedErrorText, messageError, translatedError, type LocalizedError } from '@/i18n/localizedError'

const PICKER_SCOPE = 'workspace-management-root'

export function WorkspaceDetailPage() {
  const { t } = useTranslation('workspaces')
  const { workspaceId } = useParams<{ workspaceId: string }>()
  const workspaces = useWorkspaces()
  const navigate = useNavigate()

  if (workspaces.isLoading) {
    return <DetailSkeleton label={t('detail.loading')} />
  }
  if (workspaces.error) {
    return (
      <PageState
        variant="error"
        title={t('detail.loadError', { error: String(workspaces.error) })}
      />
    )
  }

  const workspace = (workspaces.data ?? []).find((w) => w.id === workspaceId)
  if (!workspace) {
    return <PageState title={t('detail.notFound')} />
  }

  return (
    <WorkspaceDetail
      key={workspace.id}
      workspace={workspace}
      onDeleted={() => void navigate('/workspaces')}
    />
  )
}

interface WorkspaceDetailProps {
  workspace: WorkspaceRead
  onDeleted: () => void
}

function WorkspaceDetail({ workspace, onDeleted }: WorkspaceDetailProps) {
  const { t } = useTranslation(['workspaces', 'common'])
  const updateWorkspace = useUpdateWorkspace(workspace.id)
  const deleteWorkspace = useDeleteWorkspace()
  const [name, setName] = useState(workspace.name)
  const [localPath, setLocalPath] = useState(workspace.local_path ?? '')
  const [sandboxRef, setSandboxRef] = useState(workspace.sandbox_ref ?? '')
  const [error, setError] = useState<LocalizedError | null>(null)
  const [confirmOpen, setConfirmOpen] = useState(false)

  useEffect(() => {
    setName(workspace.name)
    setLocalPath(workspace.local_path ?? '')
    setSandboxRef(workspace.sandbox_ref ?? '')
  }, [workspace.local_path, workspace.name, workspace.sandbox_ref])

  const trimmedName = name.trim()
  const trimmedLocalPath = localPath.trim()
  const trimmedSandboxRef = sandboxRef.trim()
  const localPathInvalid =
    workspace.backend_type === 'local' &&
    trimmedLocalPath.length > 0 &&
    !looksAbsolute(trimmedLocalPath)
  const dirty =
    trimmedName !== workspace.name ||
    trimmedLocalPath !== (workspace.local_path ?? '') ||
    trimmedSandboxRef !== (workspace.sandbox_ref ?? '')
  const canSave =
    dirty &&
    trimmedName.length > 0 &&
    !localPathInvalid &&
    !updateWorkspace.isPending &&
    !deleteWorkspace.isPending

  const onPathChange = (nextPath: string) => {
    setLocalPath(nextPath)
    saveRememberedPrefix(PICKER_SCOPE, nextPath)
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
      return
    }
    if (result.kind === 'cancelled') return
    if (result.kind === 'fallback') {
      setError(translatedError('workspaces:validation.pickerUnavailable'))
      return
    }
    setError(messageError(result.message))
  }

  const onSave = () => {
    const payload: WorkspaceUpdate = { name: trimmedName }
    if (workspace.backend_type === 'local') {
      payload.local_path = trimmedLocalPath
    } else {
      payload.sandbox_ref = trimmedSandboxRef || null
    }
    setError(null)
    updateWorkspace.mutate(payload, {
      onError: (err) => {
        setError(err instanceof ApiError ? messageError(err.message) : translatedError('workspaces:errors.update'))
      },
    })
  }

  return (
    <DetailShell
      title={workspace.name}
      subtitle={
        <>
          <Badge variant="outline">{workspace.backend_type}</Badge>
          <Badge variant={workspace.status === 'active' ? 'default' : 'secondary'}>
            {formatResourceStatus(workspace.status, t)}
          </Badge>
        </>
      }
      actions={
        <Button
          size="sm"
          variant="destructive"
          onClick={() => setConfirmOpen(true)}
          disabled={updateWorkspace.isPending || deleteWorkspace.isPending}
        >
          {deleteWorkspace.isPending ? t('common:actions.deleting') : t('common:actions.delete')}
        </Button>
      }
    >
      <div className="space-y-10">
        <SettingsSection
          title={t('workspaces:detail.title')}
          aside={
            <Button size="sm" onClick={onSave} disabled={!canSave}>
              {updateWorkspace.isPending ? t('common:actions.saving') : t('common:actions.save')}
            </Button>
          }
        >
          <SettingsRow label={t('workspaces:fields.name')} htmlFor="workspace-edit-name" stacked>
            <Input
              id="workspace-edit-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              className="w-full"
            />
          </SettingsRow>
          {workspace.backend_type === 'local' ? (
            <SettingsRow
              label={t('workspaces:fields.backendLocalPath')}
              description={t('workspaces:fields.backendLocalPathDescription')}
              htmlFor="workspace-edit-path"
              stacked
            >
              <div className="flex w-full gap-2">
                <Input
                  id="workspace-edit-path"
                  value={localPath}
                  onChange={(event) => onPathChange(event.target.value)}
                  className={localPathInvalid ? 'border-destructive' : undefined}
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void onPickFolder()}
                >
                  {t('workspaces:actions.pickFolder')}
                </Button>
              </div>
              {localPathInvalid ? (
                <p className="text-xs text-destructive">
                  {t('workspaces:validation.absolutePath')}
                </p>
              ) : null}
            </SettingsRow>
          ) : (
            <SettingsRow label={t('workspaces:fields.sandboxRef')} htmlFor="workspace-edit-sandbox" stacked>
              <Input
                id="workspace-edit-sandbox"
                value={sandboxRef}
                onChange={(event) => setSandboxRef(event.target.value)}
                className="w-full"
              />
            </SettingsRow>
          )}
          {localizedErrorText(error, t) ? (
            <p className="py-2 text-sm text-destructive" role="alert">
              {localizedErrorText(error, t)}
            </p>
          ) : null}
        </SettingsSection>

        <WorkspaceUsageSection workspaceId={workspace.id} />
      </div>

      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={t('workspaces:detail.deleteTitle', { name: workspace.name })}
        description={t('workspaces:detail.deleteDescription')}
        confirmLabel={t('common:actions.delete')}
        destructive
        onConfirm={async () => {
          await deleteWorkspace.mutateAsync(workspace.id)
          onDeleted()
        }}
      />
    </DetailShell>
  )
}

interface WorkspaceUsageSectionProps {
  workspaceId: string
}

/**
 * Which groups and agents are bound to this workspace, with a way to act on
 * each. A read-only list makes you hunt for the entity's own screen to change
 * anything, which is the wrong direction of travel when you got here by asking
 * "who is using this folder?".
 *
 * A group can be unbound in place. An agent cannot: the API requires every
 * agent to have a workspace, so the only honest action is to go and rebind it.
 */
function WorkspaceUsageSection({ workspaceId }: WorkspaceUsageSectionProps) {
  const { t } = useTranslation(['workspaces', 'common'])
  const groups = useGroups()
  const agents = useAgents()

  const boundGroups = (groups.data ?? []).filter((g) => g.workspace_id === workspaceId)
  const boundAgents = (agents.data ?? []).filter((a) => a.workspace_id === workspaceId)
  const isLoading = groups.isLoading || agents.isLoading

  return (
    <SettingsSection
      title={t('workspaces:detail.usedBy')}
      description={t('workspaces:detail.usedByDescription')}
    >
      <div className="py-4">
        {isLoading ? (
          <p className="text-sm text-muted-foreground">{t('workspaces:detail.loadingUsage')}</p>
        ) : boundGroups.length === 0 && boundAgents.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {t('workspaces:detail.unused')}
          </p>
        ) : (
          <ul className="space-y-1.5">
            {boundGroups.map((group) => (
              <BoundGroupRow key={group.id} group={group} />
            ))}
            {boundAgents.map((agent) => (
              <li key={agent.id} className="flex items-center gap-2 text-sm">
                <Bot className="h-4 w-4 shrink-0 text-muted-foreground" />
                <Link to={`/agents/${agent.id}`} className="truncate hover:underline">
                  {agent.name}
                </Link>
                <Badge variant="outline" className="text-[10px]">
                  {t('workspaces:detail.agent')}
                </Badge>
                <Link
                  to={`/agents/${agent.id}`}
                  className="ml-auto shrink-0 text-xs text-muted-foreground hover:underline"
                >
                  {t('workspaces:detail.rebind')}
                </Link>
              </li>
            ))}
          </ul>
        )}
      </div>
    </SettingsSection>
  )
}

function BoundGroupRow({ group }: { group: GroupRead }) {
  const { t } = useTranslation(['workspaces', 'common'])
  const update = useUpdateGroup(group.id)
  const [error, setError] = useState<LocalizedError | null>(null)

  return (
    <li className="flex items-center gap-2 text-sm">
      <Users className="h-4 w-4 shrink-0 text-muted-foreground" />
      <Link to={`/groups/${group.id}`} className="truncate hover:underline">
        {group.name}
      </Link>
      <Badge variant="outline" className="text-[10px]">
        {t('workspaces:detail.group')}
      </Badge>
      {error ? (
        <span className="truncate text-xs text-destructive" role="alert">
          {localizedErrorText(error, t)}
        </span>
      ) : null}
      <Button
        size="sm"
        variant="ghost"
        className="ml-auto h-7 shrink-0 text-xs"
        disabled={update.isPending}
        onClick={() => {
          setError(null)
          update.mutate(
            { workspace_id: null },
            {
              onError: (err) =>
                setError(
                  err instanceof ApiError
                    ? messageError(err.message)
                    : translatedError('workspaces:errors.unbind'),
                ),
            },
          )
        }}
      >
        {update.isPending ? t('common:actions.saving') : t('workspaces:detail.unbind')}
      </Button>
    </li>
  )
}
