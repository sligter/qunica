import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { Link, useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { z } from 'zod'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { WorkspaceField } from '@/components/agents/WorkspaceField'
import { useAgents } from '@/hooks/useAgents'
import { useCreateGroup } from '@/hooks/useCreateGroup'
import { useSystemSettings } from '@/hooks/useSystemSettings'
import { useGroupTemplates } from '@/hooks/useGroupTemplates'
import { ApiError } from '@/lib/api-v2/client'
import { cn } from '@/lib/utils'
import type { GroupCommunicationMode } from '@/types/api'

const communicationModeKeys = {
  mesh: { label: 'settings.modes.mesh', description: 'settings.modes.meshDescription' },
  star: { label: 'settings.modes.star', description: 'settings.modes.starDescription' },
  hierarchical: {
    label: 'settings.modes.hierarchical',
    description: 'settings.modes.hierarchicalDescription',
  },
  ring: { label: 'settings.modes.ring', description: 'settings.modes.ringDescription' },
} as const satisfies Record<GroupCommunicationMode, { label: string; description: string }>

const communicationModes = Object.keys(communicationModeKeys) as GroupCommunicationMode[]

const schema = z.object({
  name: z.string().min(1, 'required').max(100, 'nameTooLong'),
  description: z.string().optional(),
  announcement: z.string().optional(),
  communication_mode: z.enum(['mesh', 'star', 'hierarchical', 'ring']),
  free_speech: z.boolean(),
  allow_agent_free_mention: z.boolean(),
})

type FormValues = z.infer<typeof schema>

interface GroupFormDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

interface GroupFormDialogBodyProps {
  onOpenChange: (open: boolean) => void
}

/**
 * Create-group form body. Lives inside `DialogContent`, so the agent list and
 * system settings are fetched when the dialog opens rather than when the sidebar
 * that owns it renders — and each open starts from a fresh mount, which is what
 * resets the form.
 */
function GroupFormDialogBody({ onOpenChange }: GroupFormDialogBodyProps) {
  const { t } = useTranslation(['groups', 'common'])
  const navigate = useNavigate()
  const agents = useAgents()
  const settings = useSystemSettings()
  const templates = useGroupTemplates()
  const createGroup = useCreateGroup()
  const [selectedAgentIds, setSelectedAgentIds] = useState<string[]>([])
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState('')
  const [selectedTemplateId, setSelectedTemplateId] = useState('')
  const [submitError, setSubmitError] = useState<string | null | undefined>(undefined)

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: '',
      description: '',
      announcement: '',
      communication_mode: 'mesh',
      free_speech: false,
      allow_agent_free_mention: true,
    },
  })

  const toggleAgent = (id: string) => {
    setSelectedAgentIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    )
  }

  const rootConfigured = Boolean(settings.data?.group_workspace_root)

  const applyTemplate = (templateId: string) => {
    setSelectedTemplateId(templateId)
    const template = templates.data?.find((item) => item.id === templateId)
    if (!template) {
      form.setValue('description', '')
      form.setValue('announcement', '')
      form.setValue('communication_mode', 'mesh')
      form.setValue('free_speech', false)
      form.setValue('allow_agent_free_mention', true)
      setSelectedAgentIds([])
      return
    }
    const config = template.config
    form.setValue('description', config.description ?? '')
    form.setValue('announcement', config.announcement ?? '')
    form.setValue('communication_mode', config.communication_mode)
    form.setValue('free_speech', config.free_speech)
    form.setValue('allow_agent_free_mention', config.allow_agent_free_mention)
    const available = new Set(agents.data?.map((agent) => agent.id) ?? [])
    setSelectedAgentIds(config.initial_agents.filter((agentId) => available.has(agentId)))
  }

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(undefined)
    try {
      const created = await createGroup.mutateAsync({
        name: values.name,
        template_id: selectedTemplateId || undefined,
        description: values.description ?? null,
        announcement: values.announcement ?? null,
        communication_mode: values.communication_mode,
        free_speech: values.free_speech,
        allow_agent_free_mention: values.allow_agent_free_mention,
        workspace_id: selectedWorkspaceId || undefined,
        initial_agents: selectedAgentIds,
      })
      onOpenChange(false)
      void navigate(`/groups/${created.id}`)
    } catch (err) {
      setSubmitError(err instanceof ApiError ? err.message : null)
    }
  })

  return (
    <>
        <DialogHeader className="shrink-0">
          <DialogTitle>{t('create.title')}</DialogTitle>
          <DialogDescription>{t('create.description')}</DialogDescription>
        </DialogHeader>

        <form onSubmit={onSubmit} className="flex min-h-0 flex-1 flex-col gap-4">
          <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pr-1">
          <div className="space-y-1.5">
            <Label htmlFor="gd-name">{t('create.name')}</Label>
            <Input id="gd-name" {...form.register('name')} />
            {form.formState.errors.name && (
              <p className="text-xs text-destructive">
                {t(`create.${form.formState.errors.name.message}`)}
              </p>
            )}
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="gd-template">{t('create.template')}</Label>
            <select
              id="gd-template"
              value={selectedTemplateId}
              onChange={(event) => applyTemplate(event.target.value)}
              disabled={templates.isLoading || agents.isLoading}
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm focus:outline-none focus:ring-1 focus:ring-ring"
            >
              <option value="">{t('create.noTemplate')}</option>
              {templates.data?.map((template) => (
                <option key={template.id} value={template.id}>{template.name}</option>
              ))}
            </select>
            <p className="text-xs text-muted-foreground">{t('create.templateDescription')}</p>
          </div>

          <div className="rounded-md border border-border bg-muted/30 p-3 text-xs">
            <p className="font-medium">{t('create.workspace')}</p>
            <p className="mb-2 text-muted-foreground">
              {t('create.workspaceDescription')}
            </p>
            <WorkspaceField value={selectedWorkspaceId} onChange={setSelectedWorkspaceId} />
            {selectedWorkspaceId ? (
              <p className="mt-2 text-muted-foreground">
                {t('create.workspaceSelected')}
              </p>
            ) : settings.isLoading ? (
              <p className="text-muted-foreground">{t('create.workspaceLoading')}</p>
            ) : rootConfigured ? (
              <p className="text-muted-foreground">
                {t('create.workspaceAutoCreate')}{' '}
                <code>{settings.data?.group_workspace_root}</code>.
              </p>
            ) : (
              <p className="text-destructive">
                {t('create.workspaceMissing')}{' '}
                <Link
                  className="underline"
                  to="/settings"
                  onClick={() => onOpenChange(false)}
                >
                  {t('create.workspaceSettingsLink')}
                </Link>{' '}
                {t('create.workspaceMissingSuffix')}
              </p>
            )}
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="gd-description">{t('create.optionalDescription')}</Label>
            <Input id="gd-description" {...form.register('description')} />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="gd-announcement">{t('create.optionalAnnouncement')}</Label>
            <Textarea
              id="gd-announcement"
              rows={2}
              placeholder={t('create.announcementPlaceholder')}
              {...form.register('announcement')}
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="gd-communication-mode">{t('create.communicationMode')}</Label>
            <select
              id="gd-communication-mode"
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm focus:outline-none focus:ring-1 focus:ring-ring"
              {...form.register('communication_mode')}
            >
              {communicationModes.map((mode) => (
                <option key={mode} value={mode}>
                  {t(communicationModeKeys[mode].label)}
                </option>
              ))}
            </select>
            <p className="text-xs text-muted-foreground">
              {
                t(communicationModeKeys[form.watch('communication_mode')].description)
              }
            </p>
          </div>

          <div className="space-y-1.5">
            <Label>{t('create.initialAgents')}</Label>
            {agents.data && agents.data.length === 0 && (
              <p className="text-xs text-muted-foreground">
                {t('create.noAgents')}
              </p>
            )}
            {agents.data && agents.data.length > 0 && (
              <ul className="flex flex-wrap gap-2">
                {agents.data.map((a) => {
                  const checked = selectedAgentIds.includes(a.id)
                  return (
                    <li key={a.id}>
                      <button
                        type="button"
                        onClick={() => toggleAgent(a.id)}
                        className={cn(
                          'rounded-md border px-3 py-1 text-xs transition-colors',
                          checked
                            ? 'border-primary bg-primary text-primary-foreground'
                            : 'border-border bg-background hover:bg-muted',
                        )}
                      >
                        {a.name}
                      </button>
                    </li>
                  )
                })}
              </ul>
            )}
          </div>

          {submitError !== undefined && (
            <p className="text-sm text-destructive" role="alert">
              {submitError
                ? t('errors.createDetail', { message: submitError })
                : t('errors.create')}
            </p>
          )}

          </div>

          <DialogFooter className="shrink-0">
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
              disabled={createGroup.isPending}
            >
              {t('common:actions.cancel')}
            </Button>
            <Button
              type="submit"
              disabled={
                createGroup.isPending ||
                (!selectedWorkspaceId && (settings.isLoading || !rootConfigured))
              }
            >
              {createGroup.isPending ? t('create.creating') : t('create.submit')}
            </Button>
          </DialogFooter>
        </form>
    </>
  )
}

/**
 * Modal dialog for creating a new group. Editing an existing group is handled
 * on the group manage page (`/groups/:groupId/manage`), reached from the chat
 * header's gear icon — out of scope here.
 */
export function GroupFormDialog({ open, onOpenChange }: GroupFormDialogProps) {
  const { t } = useTranslation('common')

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        closeLabel={t('actions.close')}
        className="flex max-h-[85vh] w-[95vw] flex-col gap-4 overflow-hidden sm:max-w-2xl"
      >
        <GroupFormDialogBody onOpenChange={onOpenChange} />
      </DialogContent>
    </Dialog>
  )
}
