import type { ReactElement } from 'react'

import { cn } from '@/lib/utils'
import type { AgentAvatarPreset, AgentAvatarPresetId } from '@/lib/agentAvatar'

/**
 * Artwork for the preset agent avatars — flat, two-tone marks in the spirit of
 * a risograph print: one saturated pass in the preset's palette slot, one ink
 * pass that tracks `--color-foreground` so a mark inverts with the theme
 * instead of needing a second set of drawings. Tones are mixed against the card
 * surface rather than layered with opacity, so a mark keeps its value on every
 * surface it lands on (list row, popover, chat gutter).
 *
 * Every mark is drawn full-bleed in a 48×48 box and cropped by the round Avatar
 * shell, and is built from at most four large shapes — enough silhouette to
 * still read at the 24px chat size.
 */
interface MarkTones {
  /** The palette slot at full strength — carries the silhouette. */
  bold: string
  /** Two thirds toward the surface — secondary masses. */
  mid: string
  /** A wash of the slot — lit faces, underprints. */
  soft: string
  /** Theme foreground; near-black in light, near-white in dark. */
  ink: string
}

const BLOOM_PETAL = 'M24 24C29.5 15.5 29.5 6.5 24 2.5C18.5 6.5 18.5 15.5 24 24Z'
const BLOOM_TURNS = [0, 72, 144, 216, 288]
const EMBER_STAR = 'M22 4C24 17 30 23 43 25C30 27 24 33 22 46C20 33 14 27 1 25C14 23 20 17 22 4Z'
/** Lanes for the woven lattice, in the mark's pre-rotation coordinates. */
const LOOM_LANES = [8, 21, 34]

const MARKS: Record<AgentAvatarPresetId, (tones: MarkTones) => ReactElement> = {
  // A lamp throwing three concentric signals over a tower cropped by the disc.
  beacon: ({ bold, mid, soft, ink }) => (
    <>
      <path d="M4.3 8.8A21 21 0 0 1 43.7 8.8" fill="none" stroke={soft} strokeWidth="2.6" strokeLinecap="round" />
      <path d="M9 10.5A16 16 0 0 1 39 10.5" fill="none" stroke={mid} strokeWidth="2.6" strokeLinecap="round" />
      <path d="M13.7 12.2A11 11 0 0 1 34.3 12.2" fill="none" stroke={bold} strokeWidth="2.8" strokeLinecap="round" />
      <path d="M16.5 48L20.5 23H27.5L31.5 48Z" fill={ink} opacity="0.85" />
      <circle cx="24" cy="17" r="5.6" fill={bold} />
    </>
  ),
  // Heraldic shield, folded down the middle so one face catches the light.
  crest: ({ bold, soft, ink }) => (
    <>
      <path d="M24 2L43 9V24C43 35 35.5 42.5 24 47C12.5 42.5 5 35 5 24V9Z" fill={bold} />
      <path d="M24 2L5 9V24C5 35 12.5 42.5 24 47Z" fill={soft} />
      <path d="M13.5 24.5L24 17L34.5 24.5" fill="none" stroke={ink} strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" opacity="0.85" />
      <path d="M16.5 33.5L24 28L31.5 33.5" fill="none" stroke={ink} strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" opacity="0.45" />
    </>
  ),
  // Three swells stacked front-to-back under a low moon.
  tide: ({ bold, mid, soft, ink }) => (
    <>
      <circle cx="33" cy="13" r="5.5" fill={ink} opacity="0.8" />
      <path d="M-4 24C6 17.5 14 30.5 24 24C34 17.5 42 30.5 52 24L52 54L-4 54Z" fill={soft} />
      <path d="M-4 32C6 25.5 14 38.5 24 32C34 25.5 42 38.5 52 32L52 54L-4 54Z" fill={mid} />
      <path d="M-4 40C6 33.5 14 46.5 24 40C34 33.5 42 46.5 52 40L52 54L-4 54Z" fill={bold} />
    </>
  ),
  // Basket weave on the diagonal: the accent lanes surface at alternating crossings.
  loom: ({ bold, ink }) => (
    <g transform="rotate(45 24 24)">
      {LOOM_LANES.map((x) => (
        <rect key={`warp-${x}`} x={x} y="-16" width="5.5" height="80" fill={bold} />
      ))}
      {LOOM_LANES.map((y) => (
        <rect key={`weft-${y}`} x="-16" y={y} width="80" height="5.5" fill={ink} opacity="0.8" />
      ))}
      {LOOM_LANES.flatMap((x, column) =>
        LOOM_LANES.filter((_, row) => (column + row) % 2 === 0).map((y) => (
          <rect key={`over-${x}-${y}`} x={x} y={y} width="5.5" height="5.5" fill={bold} />
        )),
      )}
    </g>
  ),
  // One beam in, a spread of wavelengths out — the fan is drawn first so it
  // reads as emerging from behind the glass.
  prism: ({ bold, mid, soft, ink }) => (
    <>
      <path d="M-2 20H20" fill="none" stroke={ink} strokeWidth="2.6" strokeLinecap="round" opacity="0.85" />
      <path d="M26 18L52 9" fill="none" stroke={bold} strokeWidth="2.8" strokeLinecap="round" />
      <path d="M26 18L52 19" fill="none" stroke={mid} strokeWidth="2.8" strokeLinecap="round" />
      <path d="M26 18L52 30" fill="none" stroke={soft} strokeWidth="2.8" strokeLinecap="round" />
      <path d="M24 7L41 36H7Z" fill={soft} stroke={bold} strokeWidth="2.6" strokeLinejoin="round" />
    </>
  ),
  // A cratered body with its ring passing behind, then in front along the near edge.
  orbit: ({ bold, mid, soft, ink }) => (
    <>
      <ellipse cx="24" cy="25" rx="21.5" ry="7.5" fill="none" stroke={ink} strokeWidth="2.4" opacity="0.75" transform="rotate(-20 24 25)" />
      <circle cx="21" cy="22" r="11.5" fill={bold} />
      <circle cx="17" cy="18" r="3.4" fill={soft} />
      <circle cx="25.5" cy="27" r="2.1" fill={soft} />
      <path d="M2.5 25A21.5 7.5 0 0 0 45.5 25" fill="none" stroke={ink} strokeWidth="2.4" opacity="0.75" transform="rotate(-20 24 25)" />
      <circle cx="38" cy="11" r="3.4" fill={mid} />
    </>
  ),
  // A five-petal rosette over a half-turn underprint — ten petals from one shape.
  bloom: ({ bold, soft, ink }) => (
    <>
      <g fill={soft} transform="rotate(36 24 24)">
        {BLOOM_TURNS.map((angle) => (
          <path key={angle} d={BLOOM_PETAL} transform={`rotate(${angle} 24 24)`} />
        ))}
      </g>
      <g fill={bold}>
        {BLOOM_TURNS.map((angle) => (
          <path key={angle} d={BLOOM_PETAL} transform={`rotate(${angle} 24 24)`} />
        ))}
      </g>
      <circle cx="24" cy="24" r="4.4" fill={ink} opacity="0.85" />
    </>
  ),
  // Four-point spark doubled on the diagonal, with a second smaller catch.
  ember: ({ bold, soft, ink }) => (
    <>
      <path d={EMBER_STAR} fill={soft} transform="rotate(45 22 25)" />
      <path d={EMBER_STAR} fill={bold} />
      <path d="M37 3C37.8 8 40 10.2 45 11C40 11.8 37.8 14 37 19C36.2 14 34 11.8 29 11C34 10.2 36.2 8 37 3Z" fill={ink} opacity="0.85" />
    </>
  ),
}

interface AgentAvatarArtProps {
  preset: AgentAvatarPreset
  className?: string
}

/** Renders one preset mark, sized to fill whatever round shell contains it. */
export function AgentAvatarArt({ preset, className }: AgentAvatarArtProps) {
  const { accent } = preset
  const surface = 'var(--color-card)'
  const tones: MarkTones = {
    bold: accent,
    mid: `color-mix(in oklab, ${accent} 68%, ${surface})`,
    soft: `color-mix(in oklab, ${accent} 38%, ${surface})`,
    ink: 'var(--color-foreground)',
  }

  return (
    <svg
      viewBox="0 0 48 48"
      aria-hidden="true"
      focusable="false"
      className={cn('h-full w-full', className)}
    >
      <rect width="48" height="48" fill={`color-mix(in oklab, ${accent} 15%, ${surface})`} />
      {MARKS[preset.id](tones)}
    </svg>
  )
}
