import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import {
  EDIT_PROVIDER_FORM_ID,
  EditProviderForm,
} from '@/components/providers/EditProviderForm'
import { DetailShell } from '@/components/layout/DetailShell'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Field, FieldGrid } from '@/components/ui/field'
import { PageState } from '@/components/ui/page-state'
import { Section } from '@/components/ui/section'
import { useDeleteProvider, useProvider } from '@/hooks/useProviders'
import { useEditSaveGuard } from '@/hooks/useEditSaveGuard'
import { formatNumber } from '@/lib/format'
import type { Language } from '@/i18n'
import { formatResourceStatus } from '@/i18n/resourceStatus'

export function ProviderDetailPage() {
  const { t, i18n } = useTranslation(['providers', 'common'])
  const { providerId } = useParams<{ providerId: string }>()
  const provider = useProvider(providerId)
  const del = useDeleteProvider()
  const navigate = useNavigate()
  const [editing, setEditing] = useState(false)
  const [saving, setSaving] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)
  const saveReady = useEditSaveGuard(editing)

  if (provider.isLoading) {
    return <PageState variant="loading" title={t('providers:detail.loading')} />
  }
  if (provider.error) {
    return (
      <PageState
        variant="error"
        title={t('providers:detail.loadError', { error: String(provider.error) })}
      />
    )
  }
  if (!provider.data) {
    return <PageState title={t('providers:detail.notFound')} />
  }

  const p = provider.data

  if (editing) {
    return (
      <DetailShell
        title={t('providers:detail.editTitle', { name: p.name })}
        actions={
          <>
            <Button
              size="sm"
              type="submit"
              form={EDIT_PROVIDER_FORM_ID}
              disabled={!saveReady || saving}
            >
              {saving
                ? t('providers:actions.saving')
                : t('providers:form.saveChanges')}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
              {t('common:actions.cancel')}
            </Button>
          </>
        }
      >
        <EditProviderForm
          provider={p}
          onSavingChange={setSaving}
          onSaved={() => setEditing(false)}
        />
      </DetailShell>
    )
  }

  return (
    <DetailShell
      title={p.name}
      subtitle={`${p.kind} · ${p.default_model}`}
      actions={
        <>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              setSaving(false)
              setEditing(true)
            }}
          >
            {t('common:actions.edit')}
          </Button>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => setConfirmOpen(true)}
            disabled={del.isPending}
          >
            {del.isPending ? t('common:actions.deleting') : t('common:actions.delete')}
          </Button>
        </>
      }
    >
      <div className="space-y-8">
        <FieldGrid columns={4}>
          <Field label={t('providers:fields.kind')} value={p.kind} />
          <Field label={t('providers:fields.defaultModel')} value={p.default_model} />
          <Field
            label={t('providers:fields.contextWindow')}
            value={
              p.context_window_tokens !== null
                ? formatNumber(p.context_window_tokens, i18n.resolvedLanguage as Language)
                : t('providers:states.auto')
            }
          />
          <Field
            label={t('providers:fields.outputReserve')}
            value={
              p.context_output_reserve_ratio !== null
                ? `${Math.round(p.context_output_reserve_ratio * 100)}%`
                : '30%'
            }
          />
          <Field label={t('providers:fields.baseUrl')} value={p.base_url ?? '-'} />
          <Field label={t('providers:fields.apiKey')} value={p.api_key_masked} mono />
          <Field label={t('providers:fields.status')}>
            <Badge variant={p.status === 'active' ? 'default' : 'secondary'} className="mt-1">
              {formatResourceStatus(p.status, t)}
            </Badge>
          </Field>
        </FieldGrid>

        <Section title={t('providers:models.title')} as="h3">
          <div className="grid gap-3 sm:grid-cols-2">
            {(p.models ?? []).map((model) => (
              <div
                key={model.id}
                className="rounded-md border border-border bg-card p-3 text-sm"
              >
                <div className="flex items-center gap-2">
                  <span className="font-medium">{model.id}</span>
                  {model.id === p.default_model && (
                    <Badge variant="secondary">{t('providers:models.default')}</Badge>
                  )}
                </div>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t('providers:fields.contextWindow')}: {
                    model.context_window_tokens !== null
                      ? formatNumber(model.context_window_tokens, i18n.resolvedLanguage as Language)
                      : t('providers:states.auto')
                  } · {t('providers:fields.outputReserve')}: {
                    model.context_output_reserve_ratio !== null
                      ? `${Math.round(model.context_output_reserve_ratio * 100)}%`
                      : '30%'
                  }
                </p>
              </div>
            ))}
          </div>
        </Section>

        {p.description && (
          <Section title={t('providers:detail.description')} as="h3">
            <p className="whitespace-pre-wrap text-sm leading-relaxed">{p.description}</p>
          </Section>
        )}
      </div>

      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={t('providers:detail.deleteTitle', { name: p.name })}
        description={t('providers:detail.deleteDescription')}
        confirmLabel={t('common:actions.delete')}
        destructive
        onConfirm={async () => {
          await del.mutateAsync(p.id)
          void navigate('/providers')
        }}
      />
    </DetailShell>
  )
}
