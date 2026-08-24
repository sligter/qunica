/**
 * Preset agent avatars. What gets stored is the `preset:*` id, never the
 * artwork — the marks stay vector, follow the theme, and can be redrawn
 * without a migration. The backend whitelists this same id list.
 *
 * Each preset owns one slot of the shared avatar palette, and the list is
 * ordered around the colour wheel (warm → cool → warm) so the picker rail
 * reads as a spectrum rather than a bag of icons.
 */
export const AGENT_AVATAR_PRESETS = [
  { id: 'beacon', value: 'preset:beacon', accent: 'var(--color-avatar-1)' },
  { id: 'crest', value: 'preset:crest', accent: 'var(--color-avatar-2)' },
  { id: 'tide', value: 'preset:tide', accent: 'var(--color-avatar-5)' },
  { id: 'loom', value: 'preset:loom', accent: 'var(--color-avatar-6)' },
  { id: 'prism', value: 'preset:prism', accent: 'var(--color-avatar-3)' },
  { id: 'orbit', value: 'preset:orbit', accent: 'var(--color-avatar-4)' },
  { id: 'bloom', value: 'preset:bloom', accent: 'var(--color-avatar-7)' },
  { id: 'ember', value: 'preset:ember', accent: 'var(--color-avatar-8)' },
] as const

export type AgentAvatarPreset = (typeof AGENT_AVATAR_PRESETS)[number]
export type AgentAvatarPresetId = AgentAvatarPreset['id']

export function findAgentAvatarPreset(value?: string | null): AgentAvatarPreset | undefined {
  return AGENT_AVATAR_PRESETS.find((preset) => preset.value === value)
}

/** Deterministic, readable tint pairs for the name-derived initials avatar. */
const AGENT_INITIALS_PALETTE = [
  'bg-avatar-1/15 text-avatar-1',
  'bg-avatar-2/15 text-avatar-2',
  'bg-avatar-3/15 text-avatar-3',
  'bg-avatar-4/15 text-avatar-4',
  'bg-avatar-5/15 text-avatar-5',
  'bg-avatar-6/15 text-avatar-6',
  'bg-avatar-7/15 text-avatar-7',
  'bg-avatar-8/15 text-avatar-8',
]

/** The tint a name gets when no preset or image is set, so pickers can preview it. */
export function agentInitialsTone(name: string): string {
  let hash = 0
  for (let i = 0; i < name.length; i += 1) {
    hash = (hash * 31 + name.charCodeAt(i)) >>> 0
  }
  return AGENT_INITIALS_PALETTE[hash % AGENT_INITIALS_PALETTE.length]!
}

export const AGENT_AVATAR_ACCEPT = 'image/png,image/jpeg,image/webp'
export const MAX_AGENT_AVATAR_FILE_BYTES = 8 * 1024 * 1024

export function validateAgentAvatarFile(file: File): 'type' | 'size' | null {
  if (!AGENT_AVATAR_ACCEPT.split(',').includes(file.type)) return 'type'
  return file.size > MAX_AGENT_AVATAR_FILE_BYTES ? 'size' : null
}

export function resizeAgentAvatar(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file)
    const image = new Image()
    const done = () => URL.revokeObjectURL(url)
    image.onerror = () => {
      done()
      reject(new Error('invalid image'))
    }
    image.onload = () => {
      const sourceSize = Math.min(image.naturalWidth, image.naturalHeight)
      if (!sourceSize) {
        done()
        reject(new Error('empty image'))
        return
      }
      const canvas = document.createElement('canvas')
      canvas.width = 256
      canvas.height = 256
      const context = canvas.getContext('2d')
      if (!context) {
        done()
        reject(new Error('canvas unavailable'))
        return
      }
      context.drawImage(
        image,
        (image.naturalWidth - sourceSize) / 2,
        (image.naturalHeight - sourceSize) / 2,
        sourceSize,
        sourceSize,
        0,
        0,
        256,
        256,
      )
      done()
      resolve(canvas.toDataURL('image/webp', 0.84))
    }
    image.src = url
  })
}
