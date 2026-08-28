import { useEffect, useState } from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Sparkles } from 'lucide-react'

import { MarkdownMessage } from '@/components/chat/MarkdownMessage'
import { SkillResourcesPanel } from '@/components/skills/SkillResourcesPanel'
import { DetailShell } from '@/components/layout/DetailShell'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { FieldError, FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { PageState } from '@/components/ui/page-state'
import { Section, SectionStack } from '@/components/ui/section'
import { DetailSkeleton } from '@/components/ui/skeleton'
import { Textarea } from '@/components/ui/textarea'
import { useDeleteSkill, useSkill, useUpdateSkill } from '@/hooks/useSkills'
import { useEditSaveGuard } from '@/hooks/useEditSaveGuard'
import { ApiError } from '@/lib/api-v2/client'
import type { SkillRead } from '@/types/api'
import { formatResourceStatus } from '@/i18n/resourceStatus'
import { localizedErrorText, messageError, translatedError, type LocalizedError } from '@/i18n/localizedError'
import { errorMessage } from '@/lib/utils'

const EDIT_SKILL_FORM_ID = 'edit-skill-form'

export function SkillDetailPage() {
  const { t } = useTranslation(['skills', 'common'])
  const { skillId } = useParams<{ skillId: string }>()
  const skill = useSkill(skillId)
  const del = useDeleteSkill()
  const navigate = useNavigate()
  const [searchParams, setSearchParams] = useSearchParams()
  // Deep link: /skills/:id?edit=1 opens straight into the edit form, so the
  // hover pencil on a gallery card is one click to editable fields.
  const [editing, setEditing] = useState(searchParams.get('edit') === '1')
  const [saving, setSaving] = useState(false)
  const [canSave, setCanSave] = useState(false)
  const saveReady = useEditSaveGuard(editing)
  const [confirmOpen, setConfirmOpen] = useState(false)

  useEffect(() => {
    if (editing) {
      setSearchParams(new URLSearchParams({ edit: '1' }), { replace: true })
    } else {
      setSearchParams({}, { replace: true })
    }
  }, [editing, setSearchParams])

  if (skill.isLoading) {
    return <DetailSkeleton label={t('skills:detail.loading')} />
  }
  if (skill.error) {
    return (
      <PageState
        variant="error"
        title={t('skills:detail.loadError', { error: errorMessage(skill.error) })}
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
        <div className="flex flex-wrap items-center gap-2">
          {s.description ? <span className="max-w-prose text-foreground/80">{s.description}</span> : null}
          <Badge variant="outline" className="gap-1 font-mono text-2xs uppercase tracking-wide">
            {t('skills:detail.sourceShort')}
            <span className="font-semibold">{s.source}</span>
          </Badge>
          <Badge
            variant={s.status === 'active' ? 'default' : 'secondary'}
            className="text-2xs"
          >
            {formatResourceStatus(s.status, t)}
          </Badge>
        </div>
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
      <div className="space-y-6">
        {/* Hero Card */}
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
          <div className="flex items-center gap-3.5">
            <span className="flex h-12 w-12 shrink-0 select-none items-center justify-center rounded-2xl bg-primary/10 text-primary shadow-xs">
              <Sparkles className="h-6 w-6" />
            </span>
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-base font-semibold">{s.name}</h2>
                <Badge variant="outline" className="text-2xs font-mono uppercase">
                  {s.source}
                </Badge>
              </div>
              <p className="text-xs text-muted-foreground mt-0.5">
                {s.description || t('skills:detail.bodyDescription')}
              </p>
            </div>
          </div>
        </div>

        <SectionStack>
          <Section
            title={t('skills:detail.body')}
            as="h3"
            description={t('skills:detail.bodyDescription')}
          >
            <article className="rounded-xl border border-border/80 bg-card p-5 shadow-xs sm:p-6">
              <MarkdownMessage content={s.body_markdown} />
            </article>
          </Section>

          <SkillResourcesPanel skill={s} />
        </SectionStack>
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
  onCanSaveChange: (canSave: boolean) => void
  onSavingChange: (saving: boolean) => void
  onSaved: () => void
}

function EditSkillForm({
  skill,
  onSaved,
  onCanSaveChange,
  onSavingChange,
}: EditSkillFormProps) {
  const { t } = useTranslation(['skills', 'common'])
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

  const nameError = trimmedName.length === 0 ? t('common:validation.required') : null
  const visibleError = localizedErrorText(error, t)

  return (
    <form
      id={EDIT_SKILL_FORM_ID}
      onSubmit={onSubmit}
      className="grid items-start gap-5 xl:grid-cols-[minmax(14rem,0.7fr)_minmax(24rem,1.3fr)]"
    >
      <FormField name="skill-edit-name" label={t('form.name')} required error={nameError}>
        {(field) => (
          <Input {...field} value={name} onChange={(e) => setName(e.target.value)} />
        )}
      </FormField>
      <FormField name="skill-edit-description" label={t('form.description')}>
        {(field) => (
          <Textarea
            {...field}
            rows={3}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder={t('form.descriptionPlaceholder')}
            className="min-h-28 max-h-64 resize-y"
          />
        )}
      </FormField>
      {visibleError ? (
        <FieldError className="text-sm xl:col-span-2">{visibleError}</FieldError>
      ) : null}
    </form>
  )
}
