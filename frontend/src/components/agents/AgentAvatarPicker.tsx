import { useRef, useState, type ChangeEvent, type ReactNode } from 'react'
import { ImagePlus, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AgentAvatarArt } from '@/components/chat/AgentAvatarArt'
import { avatarInitials } from '@/components/chat/AgentAvatar'
import { Panel } from '@/components/ui/panel'
import {
  AGENT_AVATAR_ACCEPT,
  AGENT_AVATAR_PRESETS,
  agentInitialsTone,
  resizeAgentAvatar,
  validateAgentAvatarFile,
} from '@/lib/agentAvatar'
import { cn } from '@/lib/utils'

interface AgentAvatarPickerProps {
  value: string | null
  name: string
  onChange: (value: string | null) => void
  disabled?: boolean
}

interface AvatarTileProps {
  label: string
  selected: boolean
  onClick: () => void
  disabled?: boolean
  className?: string
  children: ReactNode
}

/**
 * One choice on the rail. Every option — initials, preset, upload — is the same
 * round tile, so the selection ring is the only state the eye has to track and
 * choosing something never reflows the row.
 */
function AvatarTile({ label, selected, onClick, disabled, className, children }: AvatarTileProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={label}
      title={label}
      className={cn(
        'relative flex h-11 w-11 shrink-0 items-center justify-center overflow-hidden rounded-full',
        'transition-transform duration-200 ease-out hover:-translate-y-0.5 active:translate-y-0',
        'disabled:pointer-events-none disabled:opacity-60',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-card',
        selected && 'ring-2 ring-primary ring-offset-2 ring-offset-card',
        className,
      )}
    >
      {children}
    </button>
  )
}

export function AgentAvatarPicker({ value, name, onChange, disabled = false }: AgentAvatarPickerProps) {
  const { t } = useTranslation('agents')
  const inputRef = useRef<HTMLInputElement>(null)
  const [error, setError] = useState<string | null>(null)
  const [processing, setProcessing] = useState(false)

  const previewName = name.trim() || t('avatar.preview')
  const custom = value?.startsWith('data:image/') ? value : null
  const selectedPreset = AGENT_AVATAR_PRESETS.find((preset) => preset.value === value)
  const locked = disabled || processing
  const selectionLabel = selectedPreset
    ? t(`avatar.presets.${selectedPreset.id}`)
    : custom
      ? t('avatar.custom')
      : t('avatar.initials')

  const select = (next: string | null) => {
    setError(null)
    onChange(next)
  }

  const upload = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    event.target.value = ''
    if (!file) return
    const validation = validateAgentAvatarFile(file)
    if (validation) {
      setError(t(`avatar.${validation}`))
      return
    }
    setProcessing(true)
    setError(null)
    try {
      onChange(await resizeAgentAvatar(file))
    } catch {
      setError(t('avatar.processingFailed'))
    } finally {
      setProcessing(false)
    }
  }

  return (
    <Panel
      variant="inset"
      title={t('fields.avatar')}
      description={t('avatar.description')}
      aside={<span className="text-2xs text-muted-foreground">{selectionLabel}</span>}
    >
      <div role="group" aria-label={t('fields.avatar')} className="flex flex-wrap gap-2">
        <AvatarTile
          label={t('avatar.initials')}
          selected={!value}
          disabled={locked}
          onClick={() => select(null)}
          className={cn('text-sm font-semibold', agentInitialsTone(previewName))}
        >
          {avatarInitials(previewName)}
        </AvatarTile>

        {AGENT_AVATAR_PRESETS.map((preset) => (
          <AvatarTile
            key={preset.value}
            label={t(`avatar.presets.${preset.id}`)}
            selected={value === preset.value}
            disabled={locked}
            onClick={() => select(preset.value)}
          >
            <AgentAvatarArt preset={preset} />
          </AvatarTile>
        ))}

        <input
          ref={inputRef}
          type="file"
          accept={AGENT_AVATAR_ACCEPT}
          disabled={locked}
          className="sr-only"
          aria-label={t('avatar.upload')}
          onChange={upload}
        />
        <AvatarTile
          label={processing ? t('avatar.processing') : custom ? t('avatar.custom') : t('avatar.upload')}
          selected={Boolean(custom)}
          disabled={locked}
          onClick={() => inputRef.current?.click()}
          className={cn(
            'text-muted-foreground hover:text-foreground',
            !custom && 'border border-dashed border-border bg-muted/40',
          )}
        >
          {processing ? (
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
          ) : custom ? (
            <img src={custom} alt="" className="h-full w-full object-cover" />
          ) : (
            <ImagePlus className="h-4 w-4" aria-hidden />
          )}
        </AvatarTile>
      </div>
      {error && <p role="alert" className="mt-2 text-xs text-destructive">{error}</p>}
    </Panel>
  )
}
