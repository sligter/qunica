import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { SkillResourcesPanel } from '@/components/skills/SkillResourcesPanel'
import { DetailShell } from '@/components/layout/DetailShell'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useDeleteSkill, useSkill, useUpdateSkill } from '@/hooks/useSkills'
import { ApiError } from '@/lib/api-v2/client'
import type { SkillRead } from '@/types/api'

export function SkillDetailPage() {
  const { t } = useTranslation(['skills', 'common'])
  const { skillId } = useParams<{ skillId: string }>()
  const skill = useSkill(skillId)
  const del = useDeleteSkill()
  const navigate = useNavigate()
  const [editing, setEditing] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)

  if (skill.isLoading) {
    return <div className="p-6 text-sm text-muted-foreground">{t('skills:detail.loading')}</div>
  }
  if (skill.error) {
    return (
      <div className="p-6 text-sm text-destructive">
        {t('skills:detail.loadError', { error: String(skill.error) })}
      </div>
    )
  }
  if (!skill.data) {
    return <div className="p-6 text-sm text-muted-foreground">{t('skills:detail.notFound')}</div>
  }

  const s = skill.data

  if (editing) {
    return (
      <DetailShell
        title={t('skills:detail.editTitle', { name: s.name })}
        actions={
          <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
            {t('common:actions.cancel')}
          </Button>
        }
      >
        <EditSkillForm skill={s} onSaved={() => setEditing(false)} />
      </DetailShell>
    )
  }

  return (
    <DetailShell
      title={s.name}
      subtitle={
        <>
          {s.description ? <span>{s.description}</span> : null}
          <Badge variant="outline" className="text-[10px] uppercase">
            {t('skills:detail.source', { source: s.source })}
          </Badge>
          <Badge
            variant={s.status === 'active' ? 'default' : 'secondary'}
            className="text-[10px]"
          >
            {s.status}
          </Badge>
        </>
      }
      actions={
        <>
          <Button size="sm" variant="ghost" onClick={() => setEditing(true)}>
            {t('common:actions.edit')}
          </Button>
          <Button
            size="sm"
            variant="destructive"
            onClick={() => setConfirmOpen(true)}
            disabled={del.isPending}
          >
            {del.isPending ? t('common:actions.deleting') : t('common:actions.delete')}
          </Button>
        </>
      }
    >
      <div className="space-y-8">
        <section className="space-y-2">
          <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            {t('skills:detail.body')}
          </h3>
          <pre className="whitespace-pre-wrap break-words rounded-md border border-border bg-card p-4 text-sm">
            {s.body_markdown}
          </pre>
        </section>

        <SkillResourcesPanel skill={s} />
      </div>

      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={t('skills:detail.deleteTitle', { name: s.name })}
        description={t('skills:detail.deleteDescription')}
        confirmLabel={t('common:actions.delete')}
        destructive
        onConfirm={async () => {
          await del.mutateAsync(s.id)
          void navigate('/skills')
        }}
      />
    </DetailShell>
  )
}

interface EditSkillFormProps {
  skill: SkillRead
  onSaved: () => void
}

function EditSkillForm({ skill, onSaved }: EditSkillFormProps) {
  const { t } = useTranslation('skills')
  const update = useUpdateSkill(skill.id)
  const [name, setName] = useState(skill.name)
  const [description, setDescription] = useState(skill.description ?? '')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setName(skill.name)
    setDescription(skill.description ?? '')
  }, [skill.description, skill.name])

  const trimmedName = name.trim()
  const trimmedDescription = description.trim()
  const dirty =
    trimmedName !== skill.name || trimmedDescription !== (skill.description ?? '')
  const canSave = dirty && trimmedName.length > 0 && !update.isPending

  const onSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)
    update.mutate(
      {
        name: trimmedName,
        description: trimmedDescription || null,
      },
      {
        onSuccess: () => onSaved(),
        onError: (err) => {
          setError(err instanceof ApiError ? err.message : t('errors.update'))
        },
      },
    )
  }

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="skill-edit-name">{t('form.name')}</Label>
        <Input
          id="skill-edit-name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="max-w-xl"
        />
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="skill-edit-description">{t('form.description')}</Label>
        <Textarea
          id="skill-edit-description"
          rows={3}
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder={t('form.descriptionPlaceholder')}
          className="max-w-xl"
        />
      </div>
      {error && (
        <p className="text-sm text-destructive" role="alert">
          {error}
        </p>
      )}
      <Button type="submit" disabled={!canSave}>
        {update.isPending ? t('common:actions.saving') : t('form.saveChanges')}
      </Button>
    </form>
  )
}
