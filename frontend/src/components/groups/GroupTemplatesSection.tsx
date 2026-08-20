import { useState } from 'react'
import { Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import { SettingsRow, SettingsSection } from '@/components/ui/settings-row'
import {
  useCreateGroupTemplate,
  useDeleteGroupTemplate,
  useGroupTemplates,
} from '@/hooks/useGroupTemplates'
import type { GroupRead, GroupTemplateRead } from '@/types/api'

export function GroupTemplatesSection({ group }: { group: GroupRead }) {
  const { t } = useTranslation('groups')
  const templates = useGroupTemplates()
  const create = useCreateGroupTemplate()
  const remove = useDeleteGroupTemplate()
  const [name, setName] = useState(group.name)
  const [deleteTarget, setDeleteTarget] = useState<GroupTemplateRead | null>(null)

  return (
    <SettingsSection title={t('templates.title')} description={t('templates.description')}>
      <SettingsRow label={t('templates.saveCurrent')} description={t('templates.saveCurrentDescription')} stacked>
        <div className="flex w-full gap-2">
          <Input
            value={name}
            onChange={(event) => setName(event.target.value)}
            maxLength={100}
            aria-label={t('templates.name')}
          />
          <Button
            type="button"
            size="sm"
            disabled={!name.trim() || create.isPending}
            onClick={() => void create.mutateAsync({ name: name.trim(), group_id: group.id })}
          >
            {create.isPending ? t('templates.saving') : t('templates.save')}
          </Button>
        </div>
        {create.error ? <p className="text-sm text-destructive" role="alert">{String(create.error)}</p> : null}
      </SettingsRow>
      {templates.data?.length ? (
        <ul className="divide-y divide-border">
          {templates.data.map((template) => (
            <li key={template.id} className="flex items-center gap-3 py-2.5">
              <span className="min-w-0 flex-1 truncate text-sm font-medium">{template.name}</span>
              <span className="text-xs text-muted-foreground">
                {t('templates.agentCount', { count: template.config.initial_agents.length })}
              </span>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-muted-foreground hover:text-destructive"
                aria-label={t('templates.deleteNamed', { name: template.name })}
                onClick={() => setDeleteTarget(template)}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="py-3 text-sm text-muted-foreground">{t('templates.empty')}</p>
      )}
      <ConfirmDialog
        open={deleteTarget !== null}
        onOpenChange={(open) => { if (!open) setDeleteTarget(null) }}
        title={t('templates.deleteTitle', { name: deleteTarget?.name ?? '' })}
        description={t('templates.deleteDescription')}
        confirmLabel={t('templates.delete')}
        destructive
        onConfirm={async () => {
          if (deleteTarget) await remove.mutateAsync(deleteTarget.id)
        }}
      />
    </SettingsSection>
  )
}
