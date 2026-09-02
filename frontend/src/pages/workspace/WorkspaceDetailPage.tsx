import { useEffect, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import {
  ArrowRight,
  Bot,
  Check,
  Copy,
  Users,
} from 'lucide-react'
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
  pickFolder,
  saveRememberedPrefix,
} from '@/lib/folderPicker'
import type { GroupRead, WorkspaceRead, WorkspaceUpdate } from '@/types/api'
import { formatResourceStatus } from '@/i18n/resourceStatus'
import {
  localizedErrorText,
  messageError,
  translatedError,
  type LocalizedError,
} from '@/i18n/localizedError'
import { avatarColorClass } from '@/lib/avatarColor'
import { cn, errorMessage } from '@/lib/utils'

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
        title={t('detail.loadError', { error: errorMessage(workspaces.error) })}
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
  const [copiedPath, setCopiedPath] = useState(false)

  useEffect(() => {
    setName(workspace.name)
    setLocalPath(workspace.local_path ?? '')
    setSandboxRef(workspace.sandbox_ref ?? '')
  }, [workspace.local_path, workspace.name, workspace.sandbox_ref])

  const trimmedName = name.trim()
  const trimmedLocalPath = localPath.trim()
  const trimmedSandboxRef = sandboxRef.trim()
  const dirty =
    trimmedName !== workspace.name ||
    trimmedLocalPath !== (workspace.local_path ?? '') ||
    trimmedSandboxRef !== (workspace.sandbox_ref ?? '')
  const canSave =
    dirty &&
    trimmedName.length > 0 &&
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
      const nextPath = result.path ?? result.name
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
        setError(
          err instanceof ApiError
            ? messageError(err.message)
            : translatedError('workspaces:errors.update'),
        )
      },
    })
  }

  const activePath =
    workspace.backend_type === 'local'
      ? workspace.local_path || t('workspaces:noLocalPath')
      : workspace.sandbox_ref || t('workspaces:noSandboxReference')

  const onCopyPath = () => {
    if (!navigator.clipboard || !activePath) return
    void navigator.clipboard.writeText(activePath).then(() => {
      setCopiedPath(true)
      setTimeout(() => setCopiedPath(false), 2000)
    })
  }

  return (
    <DetailShell
      title={workspace.name}
      subtitle={
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant="outline" className="text-2xs font-mono uppercase">
            {workspace.backend_type}
          </Badge>
          <Badge
            variant={workspace.status === 'active' ? 'default' : 'secondary'}
            className="text-2xs"
          >
            {formatResourceStatus(workspace.status, t)}
          </Badge>
        </div>
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
      <div className="space-y-6">
        {/* Hero Card */}
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
          <div className="flex items-center gap-3.5">
            <span
              className={cn(
                'flex h-12 w-12 shrink-0 select-none items-center justify-center rounded-2xl text-base font-semibold shadow-xs',
                avatarColorClass(workspace.id),
              )}
            >
              {workspace.name.slice(0, 1).toUpperCase()}
            </span>
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-base font-semibold">{workspace.name}</h2>
                <span className="inline-block rounded-md border border-border/60 bg-muted/60 px-1.5 py-0.5 text-2xs font-mono uppercase tracking-wider text-muted-foreground">
                  {workspace.backend_type}
                </span>
              </div>
              <code className="text-xs font-mono text-muted-foreground mt-0.5 block truncate max-w-md">
                {activePath}
              </code>
            </div>
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={onCopyPath}
            className="h-8 gap-1.5 text-xs text-muted-foreground"
          >
            {copiedPath ? <Check className="h-3.5 w-3.5 text-success" /> : <Copy className="h-3.5 w-3.5" />}
            <span>{copiedPath ? t('common:actions.copied', '已复制') : t('common:actions.copy', '复制路径')}</span>
          </Button>
        </div>

        {/* Configuration Section */}
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
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void onPickFolder()}
                >
                  {t('workspaces:actions.pickFolder')}
                </Button>
              </div>
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

        {/* Usage Section */}
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
          <ul className="space-y-2">
            {boundGroups.map((group) => (
              <BoundGroupRow key={group.id} group={group} />
            ))}
            {boundAgents.map((agent) => (
              <li
                key={agent.id}
                className="flex items-center justify-between rounded-xl border border-border/80 bg-card p-3 text-sm shadow-xs transition-colors hover:border-primary/40"
              >
                <div className="flex items-center gap-2.5 min-w-0">
                  <Bot className="h-4 w-4 shrink-0 text-primary" />
                  <Link to={`/agents/${agent.id}`} className="truncate font-medium hover:underline text-foreground">
                    {agent.name}
                  </Link>
                  <Badge variant="secondary" className="text-2xs">
                    {t('workspaces:detail.agent')}
                  </Badge>
                </div>
                <Link
                  to={`/agents/${agent.id}`}
                  className="inline-flex items-center gap-1 text-xs text-primary hover:underline ml-2 shrink-0 font-medium"
                >
                  {t('workspaces:detail.rebind')}
                  <ArrowRight className="h-3 w-3" />
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
    <li className="flex items-center justify-between rounded-xl border border-border/80 bg-card p-3 text-sm shadow-xs transition-colors hover:border-primary/40">
      <div className="flex items-center gap-2.5 min-w-0">
        <Users className="h-4 w-4 shrink-0 text-primary" />
        <Link to={`/groups/${group.id}`} className="truncate font-medium hover:underline text-foreground">
          {group.name}
        </Link>
        <Badge variant="secondary" className="text-2xs">
          {t('workspaces:detail.group')}
        </Badge>
        {error ? (
          <span className="truncate text-xs text-destructive ml-2" role="alert">
            {localizedErrorText(error, t)}
          </span>
        ) : null}
      </div>
      <Button
        size="sm"
        variant="ghost"
        className="ml-auto h-7 shrink-0 text-xs text-muted-foreground hover:text-foreground"
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
