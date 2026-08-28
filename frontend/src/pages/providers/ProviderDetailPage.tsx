import { useEffect, useState } from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { Check, Key, Layers, Plug } from 'lucide-react'
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
import { DetailSkeleton } from '@/components/ui/skeleton'
import { Section } from '@/components/ui/section'
import { useDeleteProvider, useProvider } from '@/hooks/useProviders'
import { useEditSaveGuard } from '@/hooks/useEditSaveGuard'
import { useUnsavedChangesGuard } from '@/hooks/useUnsavedChangesGuard'
import { formatNumber } from '@/lib/format'
import type { Language } from '@/i18n'
import { formatResourceStatus } from '@/i18n/resourceStatus'
import { TINTED_BADGE } from '@/lib/tintedBadge'
import type { ProviderKind } from '@/types/api'
import { cn, errorMessage } from '@/lib/utils'

function kindBadgeClass(kind: ProviderKind): string {
  if (kind === 'anthropic' || kind === 'anthropic-compatible') return TINTED_BADGE.amber
  if (kind === 'gemini') return TINTED_BADGE.blue
  return TINTED_BADGE.green
}

export function ProviderDetailPage() {
  const { t, i18n } = useTranslation(['providers', 'common'])
  const { providerId } = useParams<{ providerId: string }>()
  const provider = useProvider(providerId)
  const del = useDeleteProvider()
  const navigate = useNavigate()
  const [searchParams, setSearchParams] = useSearchParams()
  // Deep link: /providers/:id?edit=1 opens straight into the edit form.
  const [editing, setEditing] = useState(searchParams.get('edit') === '1')
  const [saving, setSaving] = useState(false)
  const [dirty, setDirty] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)
  const [copiedKey, setCopiedKey] = useState(false)
  const saveReady = useEditSaveGuard(editing)
  useUnsavedChangesGuard(editing && dirty)

  useEffect(() => {
    if (editing) {
      setSearchParams(new URLSearchParams({ edit: '1' }), { replace: true })
    } else {
      setSearchParams({}, { replace: true })
    }
  }, [editing, setSearchParams])

  if (provider.isLoading) {
    return <DetailSkeleton label={t('providers:detail.loading')} />
  }
  if (provider.error) {
    return (
      <PageState
        variant="error"
        title={t('providers:detail.loadError', { error: errorMessage(provider.error) })}
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
          onDirtyChange={setDirty}
          onSavingChange={setSaving}
          onSaved={() => {
            setDirty(false)
            setEditing(false)
          }}
        />
      </DetailShell>
    )
  }

  const onCopyKey = () => {
    if (!navigator.clipboard) return
    void navigator.clipboard.writeText(p.api_key_masked).then(() => {
      setCopiedKey(true)
      setTimeout(() => setCopiedKey(false), 2000)
    })
  }

  return (
    <DetailShell
      title={p.name}
      subtitle={
        <div className="flex flex-wrap items-center gap-2">
          <span>{`${p.kind} · ${p.default_model}`}</span>
          <Badge variant={p.status === 'active' ? 'default' : 'secondary'} className="text-2xs">
            {formatResourceStatus(p.status, t)}
          </Badge>
        </div>
      }
      actions={
        <>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              setSaving(false)
              setDirty(false)
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
      <div className="space-y-6">
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
          <div className="flex items-center gap-3.5">
            <span className="flex h-12 w-12 shrink-0 select-none items-center justify-center rounded-2xl bg-primary/10 text-primary shadow-xs">
              <Plug className="h-6 w-6" />
            </span>
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-base font-semibold">{p.name}</h2>
                <span
                  className={cn(
                    'inline-block rounded-md border px-1.5 py-0.5 text-2xs font-medium leading-none',
                    kindBadgeClass(p.kind),
                  )}
                >
                  {p.kind}
                </span>
              </div>
              <p className="text-xs font-mono text-muted-foreground mt-0.5">
                {p.base_url || t('providers:kinds.openai.baseHint', '默认云端端点')}
              </p>
            </div>
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={onCopyKey}
            className="h-8 gap-1.5 text-xs text-muted-foreground"
          >
            {copiedKey ? <Check className="h-3.5 w-3.5 text-success" /> : <Key className="h-3.5 w-3.5" />}
            <span className="font-mono text-2xs">{p.api_key_masked}</span>
          </Button>
        </div>

        <FieldGrid columns={3}>
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
          <Field label={t('providers:fields.baseUrl')} value={p.base_url ?? '-'} mono />
          <Field label={t('providers:fields.apiKey')} value={p.api_key_masked} mono />
        </FieldGrid>

        <Section
          title={t('providers:models.title')}
          as="h3"
          description={t('providers:models.description')}
        >
          {/* A provider with only a default model is the normal case, so an
              empty grid here is not an error — but it was rendering as a
              heading with nothing under it. */}
          {(p.models ?? []).length === 0 ? (
            <PageState
              inset
              icon={Layers}
              title={t('providers:models.none')}
              className="px-0"
            />
          ) : null}
          <div className="grid gap-3 sm:grid-cols-2">
            {(p.models ?? []).map((model) => {
              const isDefault = model.id === p.default_model
              const contextTokens = model.context_window_tokens !== null
                ? formatNumber(model.context_window_tokens, i18n.resolvedLanguage as Language)
                : t('providers:states.auto')
              const reserveRatio = model.context_output_reserve_ratio !== null
                ? `${Math.round(model.context_output_reserve_ratio * 100)}%`
                : '30%'

              return (
                <div
                  key={model.id}
                  className="rounded-xl border border-border/80 bg-card p-4 text-sm shadow-xs transition-colors hover:border-primary/40"
                >
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2 min-w-0">
                      <span className="font-semibold truncate text-foreground">{model.id}</span>
                      {isDefault && (
                        <Badge variant="default" className="text-2xs shrink-0">
                          {t('providers:models.default')}
                        </Badge>
                      )}
                    </div>
                  </div>

                  <div className="mt-3 grid grid-cols-2 gap-2 text-xs text-muted-foreground border-t border-border/50 pt-2.5">
                    <div>
                      <span className="text-2xs uppercase text-muted-foreground/80 block">
                        {t('providers:fields.contextWindow')}
                      </span>
                      <span className="font-mono text-xs text-foreground font-medium">{contextTokens}</span>
                    </div>
                    <div>
                      <span className="text-2xs uppercase text-muted-foreground/80 block">
                        {t('providers:fields.outputReserve')}
                      </span>
                      <span className="font-mono text-xs text-foreground font-medium">{reserveRatio}</span>
                    </div>
                  </div>
                </div>
              )
            })}
          </div>
        </Section>

        {p.description && (
          <Section title={t('providers:detail.description')} as="h3">
            <p className="whitespace-pre-wrap text-sm leading-relaxed text-muted-foreground bg-card p-4 rounded-xl border border-border/80">
              {p.description}
            </p>
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
