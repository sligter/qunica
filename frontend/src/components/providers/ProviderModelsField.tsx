import { useId, useState } from 'react'
import { Check, ChevronDown, Plus, RefreshCw, Trash2 } from 'lucide-react'
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
              <ModelIdCombobox
                value={model.id}
                catalog={catalog}
                label={t('models.modelId')}
                placeholder={t('models.modelPlaceholder')}
                onChange={(id) => update(index, { id })}
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

function ModelIdCombobox({
  value,
  catalog,
  label,
  placeholder,
  onChange,
}: {
  value: string
  catalog: ModelInfo[]
  label: string
  placeholder: string
  onChange: (value: string) => void
}) {
  const listId = useId()
  const [open, setOpen] = useState(false)
  const [highlighted, setHighlighted] = useState(0)
  const [filter, setFilter] = useState('')
  const query = filter.trim().toLocaleLowerCase()
  const options = catalog.filter((model) =>
    !query || model.id.toLocaleLowerCase().includes(query) ||
    model.name.toLocaleLowerCase().includes(query),
  )

  const showOptions = () => {
    setFilter('')
    setHighlighted(Math.max(0, catalog.findIndex((model) => model.id === value)))
    setOpen(catalog.length > 0)
  }

  const select = (id: string) => {
    onChange(id)
    setFilter('')
    setOpen(false)
  }

  return (
    <div
      className="relative min-w-0 flex-1"
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false)
      }}
    >
      <Input
        role="combobox"
        aria-label={label}
        aria-autocomplete="list"
        aria-expanded={open && options.length > 0}
        aria-controls={listId}
        value={value}
        placeholder={placeholder}
        className="pr-10"
        onFocus={showOptions}
        onChange={(event) => {
          onChange(event.target.value)
          setFilter(event.target.value)
          setHighlighted(0)
          setOpen(catalog.length > 0)
        }}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            setOpen(false)
          } else if (event.key === 'ArrowDown' && options.length > 0) {
            event.preventDefault()
            if (open) setHighlighted((current) => (current + 1) % options.length)
            else showOptions()
          } else if (event.key === 'ArrowUp' && options.length > 0) {
            event.preventDefault()
            setOpen(true)
            setHighlighted((current) => (current - 1 + options.length) % options.length)
          } else if (event.key === 'Enter' && open && options[highlighted]) {
            event.preventDefault()
            select(options[highlighted].id)
          }
        }}
      />
      {catalog.length > 0 && (
        <button
          type="button"
          aria-label={placeholder}
          tabIndex={-1}
          className="absolute inset-y-px right-px flex w-9 items-center justify-center rounded-r-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => {
            if (open) setOpen(false)
            else showOptions()
          }}
        >
          <ChevronDown className={cn('h-4 w-4 transition-transform', open && 'rotate-180')} />
        </button>
      )}
      {open && options.length > 0 && (
        <div
          id={listId}
          role="listbox"
          className="absolute left-0 top-[calc(100%+0.375rem)] z-50 max-h-64 w-full max-w-xl overflow-y-auto rounded-md border border-border bg-background p-1 text-foreground shadow-xl"
        >
          {options.map((option, index) => (
            <button
              key={option.id}
              type="button"
              role="option"
              aria-selected={option.id === value}
              className={cn(
                'flex w-full items-center gap-2 rounded-sm px-2.5 py-2 text-left text-sm outline-none transition-colors',
                index === highlighted && 'bg-accent text-accent-foreground',
              )}
              onMouseEnter={() => setHighlighted(index)}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => select(option.id)}
            >
              <span className="min-w-0 flex-1 truncate">{option.name}</span>
              {option.name !== option.id && (
                <span className="max-w-48 truncate font-mono text-[11px] text-muted-foreground">
                  {option.id}
                </span>
              )}
              <Check className={cn('h-4 w-4 shrink-0', option.id !== value && 'invisible')} />
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
