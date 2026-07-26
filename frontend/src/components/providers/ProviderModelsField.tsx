import { Plus, RefreshCw, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'
import type { ModelInfo } from '@/types/api'

export interface ProviderModelDraft {
  id: string
  context_window_tokens?: number
  context_output_reserve_percent: number
}

interface ProviderModelsFieldProps {
  models: ProviderModelDraft[]
  defaultModel: string
  catalog?: ModelInfo[]
  isLoadingCatalog?: boolean
  catalogError?: boolean
  onRefreshCatalog?: () => void
  onChange: (models: ProviderModelDraft[]) => void
  onDefaultChange: (modelId: string) => void
}

export function ProviderModelsField({
  models,
  defaultModel,
  catalog = [],
  isLoadingCatalog = false,
  catalogError = false,
  onRefreshCatalog,
  onChange,
  onDefaultChange,
}: ProviderModelsFieldProps) {
  const { t } = useTranslation('providers')
  const catalogId = 'provider-model-catalog'

  const update = (index: number, patch: Partial<ProviderModelDraft>) => {
    const previous = models[index]
    const next = models.map((model, modelIndex) =>
      modelIndex === index ? { ...model, ...patch } : model,
    )
    onChange(next)
    if (patch.id !== undefined && previous.id === defaultModel) {
      onDefaultChange(patch.id)
    }
  }

  const remove = (index: number) => {
    if (models.length === 1) return
    const removed = models[index]
    const next = models.filter((_, modelIndex) => modelIndex !== index)
    onChange(next)
    if (removed.id === defaultModel) onDefaultChange(next[0]?.id ?? '')
  }

  return (
    <section className="space-y-3 rounded-md border border-border bg-card/40 p-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-medium">{t('models.title')}</h3>
          <p className="text-[11px] text-muted-foreground">{t('models.description')}</p>
        </div>
        <div className="flex gap-2">
          {onRefreshCatalog && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={isLoadingCatalog}
              onClick={onRefreshCatalog}
            >
              <RefreshCw className={cn('mr-1.5 h-3.5 w-3.5', isLoadingCatalog && 'animate-spin')} />
              {t('models.fetch')}
            </Button>
          )}
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() =>
              onChange([
                ...models,
                { id: '', context_window_tokens: undefined, context_output_reserve_percent: 30 },
              ])
            }
          >
            <Plus className="mr-1.5 h-3.5 w-3.5" />
            {t('models.add')}
          </Button>
        </div>
      </div>

      {isLoadingCatalog && <p className="text-xs text-muted-foreground">{t('models.loading')}</p>}
      {!isLoadingCatalog && catalog.length > 0 && (
        <p className="text-xs text-muted-foreground">
          {t('models.found', { count: catalog.length })}
        </p>
      )}
      {catalogError && <p className="text-xs text-destructive">{t('models.fetchError')}</p>}

      <datalist id={catalogId}>
        {catalog.map((model) => <option key={model.id} value={model.id}>{model.name}</option>)}
      </datalist>

      <div className="space-y-3">
        {models.map((model, index) => (
          <div key={index} className="rounded-md border border-border bg-background p-3">
            <div className="mb-3 flex items-center gap-3">
              <label className="flex shrink-0 items-center gap-1.5 text-xs font-medium">
                <input
                  type="radio"
                  name="provider-default-model"
                  checked={model.id !== '' && model.id === defaultModel}
                  onChange={() => onDefaultChange(model.id)}
                />
                {t('models.default')}
              </label>
              <Input
                aria-label={t('models.modelId')}
                list={catalogId}
                value={model.id}
                placeholder={t('models.modelPlaceholder')}
                onChange={(event) => update(index, { id: event.target.value })}
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                disabled={models.length === 1}
                aria-label={t('models.remove', { model: model.id || index + 1 })}
                onClick={() => remove(index)}
              >
                <Trash2 className="h-4 w-4" />
              </Button>
            </div>

            <div className="grid gap-3 sm:grid-cols-2">
              <div className="space-y-1.5">
                <Label>{t('fields.contextWindowTokens')}</Label>
                <Input
                  type="number"
                  min={1}
                  value={model.context_window_tokens ?? ''}
                  placeholder={t('form.autoFromModel')}
                  onChange={(event) =>
                    update(index, {
                      context_window_tokens: event.target.value
                        ? Number(event.target.value)
                        : undefined,
                    })
                  }
                />
              </div>
              <div className="space-y-1.5">
                <Label>{t('fields.outputReservePercent')}</Label>
                <Input
                  type="number"
                  min={1}
                  max={90}
                  value={model.context_output_reserve_percent}
                  onChange={(event) =>
                    update(index, {
                      context_output_reserve_percent: Number(event.target.value),
                    })
                  }
                />
              </div>
            </div>
          </div>
        ))}
      </div>
    </section>
  )
}
