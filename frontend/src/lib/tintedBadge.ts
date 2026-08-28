/**
 * Quiet tinted badges for typed metadata — provider kind, MCP transport.
 *
 * These used to be written as literal Tailwind palette colours
 * (`text-amber-600 dark:text-amber-400`), which put them outside the theme:
 * they ignored the app's own dark mode and drifted from the warm palette every
 * other surface in the library uses. They now point at the avatar ramp, which
 * is part of `@theme` and is defined for both light and dark, so a badge keeps
 * its identity and its contrast in each.
 */

export const TINTED_BADGE = {
  amber: 'border-avatar-2/25 bg-avatar-2/10 text-avatar-2',
  blue: 'border-avatar-3/25 bg-avatar-3/10 text-avatar-3',
  green: 'border-avatar-5/25 bg-avatar-5/10 text-avatar-5',
  violet: 'border-avatar-4/25 bg-avatar-4/10 text-avatar-4',
  teal: 'border-avatar-6/25 bg-avatar-6/10 text-avatar-6',
} as const

export type TintedBadgeTone = keyof typeof TINTED_BADGE
