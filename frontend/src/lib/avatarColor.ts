/**
 * Stable avatar tint from a seed string (id/name). Limited to the avatar-N
 * palette tokens so lists look calm and consistent across areas.
 */
const palette = [
  'bg-avatar-3 text-avatar-foreground',
  'bg-avatar-5 text-avatar-foreground',
  'bg-avatar-2 text-avatar-foreground',
  'bg-avatar-4 text-avatar-foreground',
  'bg-avatar-8 text-avatar-foreground',
  'bg-avatar-6 text-avatar-foreground',
  'bg-avatar-7 text-avatar-foreground',
  'bg-avatar-1 text-avatar-foreground',
]

export function avatarColorClass(seed: string): string {
  let h = 0
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) >>> 0
  return palette[h % palette.length]!
}
