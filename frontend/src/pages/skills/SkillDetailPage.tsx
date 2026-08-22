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
import { PageState } from '@/components/ui/page-state'
import { DetailSkeleton } from '@/components/ui/skeleton'
import { ProseBlock } from '@/components/ui/prose-block'
import { Section } from '@/components/ui/section'
import { Textarea } from '@/components/ui/textarea'
import { useDeleteSkill, useSkill, useUpdateSkill } from '@/hooks/useSkills'
import { useEditSaveGuard } from '@/hooks/useEditSaveGuard'
import { ApiError } from '@/lib/api-v2/client'
import type { SkillRead } from '@/types/api'
import { formatResourceStatus } from '@/i18n/resourceStatus'
import { localizedErrorText, messageError, translatedError, type LocalizedError } from '@/i18n/localizedError'

const EDIT_SKILL_FORM_ID = 'edit-skill-form'

export function SkillDetailPage() {
  const { t } = useTranslation(['skills', 'common'])
  const { skillId } = useParams<{ skillId: string }>()
  const skill = useSkill(skillId)
  const del = useDeleteSkill()
  const navigate = useNavigate()
  const [editing, setEditing] = useState(false)
  const [saving, setSaving] = useState(false)
  const [canSave, setCanSave] = useState(false)
  const saveReady = useEditSaveGuard(editing)
  const [confirmOpen, setConfirmOpen] = useState(false)

  if (skill.isLoading) {
    return <DetailSkeleton label={t('skills:detail.loading')} />
  }
  if (skill.error) {
    return (
      <PageState
        variant="error"
        title={t('skills:detail.loadError', { error: String(skill.error) })}
      />
    )
  }
  if (!skill.data) {
    return <PageState title={t('skills:detail.notFound')} />
  }

  const s = skill.data

  if (editing) {
    return (
      <DetailShell
        title={t('skills:detail.editTitle', { name: s.name })}
        actions={
          <>
            <Button
              size="sm"
              type="submit"
              form={EDIT_SKILL_FORM_ID}
              disabled={!saveReady || !canSave}
            >
              {saving
                ? t('common:actions.saving')
                : t('skills:form.saveChanges')}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
              {t('common:actions.cancel')}
            </Button>
          </>
        }
      >
        <EditSkillForm
          skill={s}
          onCanSaveChange={setCanSave}
          onSavingChange={setSaving}
          onSaved={() => setEditing(false)}
        />
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
            {formatResourceStatus(s.status, t)}
          </Badge>
        </>
      }
      actions={
        <>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              setCanSave(false)
              setSaving(false)
              setEditing(true)
            }}
          >
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
        <Section title={t('skills:detail.body')} as="h3">
          <ProseBlock maxHeight="lg">{s.body_markdown}</ProseBlock>
        </Section>

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
  onCanSaveChange: (canSave: boolean) => void
  onSavingChange: (saving: boolean) => void
}

function EditSkillForm({
  skill,
  onSaved,
  onCanSaveChange,
  onSavingChange,
}: EditSkillFormProps) {
  const { t } = useTranslation('skills')
  const update = useUpdateSkill(skill.id)
  const [name, setName] = useState(skill.name)
  const [description, setDescription] = useState(skill.description ?? '')
  const [error, setError] = useState<LocalizedError | null>(null)

  useEffect(() => {
    setName(skill.name)
    setDescription(skill.description ?? '')
  }, [skill.description, skill.name])

  const trimmedName = name.trim()
  const trimmedDescription = description.trim()
  const dirty =
    trimmedName !== skill.name || trimmedDescription !== (skill.description ?? '')
  const canSave = dirty && trimmedName.length > 0 && !update.isPending

  useEffect(() => {
    onCanSaveChange(canSave)
    onSavingChange(update.isPending)
  }, [canSave, onCanSaveChange, onSavingChange, update.isPending])

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
          setError(err instanceof ApiError ? messageError(err.message) : translatedError('errors.update'))
        },
      },
    )
  }

  return (
    <form
      id={EDIT_SKILL_FORM_ID}
      onSubmit={onSubmit}
      className="grid items-start gap-5 xl:grid-cols-[minmax(14rem,0.7fr)_minmax(24rem,1.3fr)]"
    >
      <div className="space-y-1.5">
        <Label htmlFor="skill-edit-name">{t('form.name')}</Label>
        <Input
          id="skill-edit-name"
          value={name}
          onChange={(e) => setName(e.target.value)}
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
          className="min-h-28 max-h-64 resize-y"
        />
      </div>
      {localizedErrorText(error, t) && (
        <p className="text-sm text-destructive xl:col-span-2" role="alert">
          {localizedErrorText(error, t)}
        </p>
      )}
    </form>
  )
}
