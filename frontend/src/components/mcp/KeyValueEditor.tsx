import { Plus, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { emptyRow, type KeyValueRow } from '@/components/mcp/keyValueRows'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

interface KeyValueEditorProps {
  rows: KeyValueRow[]
  onChange: (rows: KeyValueRow[]) => void
  keyPlaceholder: string
  valuePlaceholder: string
  addLabel: string
  /** Renders values as password fields — used for headers carrying tokens. */
  secret?: boolean
  disabled?: boolean
}

/** A small key/value list editor for env variables and HTTP headers. */
export function KeyValueEditor({
  rows,
  onChange,
  keyPlaceholder,
  valuePlaceholder,
  addLabel,
  secret = false,
  disabled = false,
}: KeyValueEditorProps) {
  const { t } = useTranslation('mcp')

  const update = (id: string, patch: Partial<KeyValueRow>) => {
    onChange(rows.map((row) => (row.id === id ? { ...row, ...patch } : row)))
  }

  return (
    <div className="space-y-2">
      {rows.map((row) => (
        <div key={row.id} className="flex items-center gap-2">
          <Input
            className="flex-1"
            value={row.key}
            placeholder={keyPlaceholder}
            disabled={disabled}
            onChange={(event) => update(row.id, { key: event.target.value })}
          />
          <Input
            className="flex-1"
            type={secret ? 'password' : 'text'}
            value={row.value}
            placeholder={valuePlaceholder}
            disabled={disabled}
            onChange={(event) => update(row.id, { value: event.target.value })}
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            disabled={disabled}
            aria-label={t('actions.removeRow')}
            onClick={() => onChange(rows.filter((item) => item.id !== row.id))}
          >
            <X className="h-4 w-4" />
          </Button>
        </div>
      ))}
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={disabled}
        onClick={() => onChange([...rows, emptyRow()])}
      >
        <Plus className="mr-1 h-3.5 w-3.5" />
        {addLabel}
      </Button>
    </div>
  )
}
