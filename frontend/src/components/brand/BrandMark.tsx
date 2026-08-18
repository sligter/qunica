import { useId } from 'react'

import { cn } from '@/lib/utils'

interface BrandMarkProps {
  className?: string
  /**
   * Plays the entrance and the slow sway. Leave it off for chrome placements —
   * a mark that moves in the sidebar is noise, not personality.
   */
  animated?: boolean
  /**
   * Accessible name. Omit it wherever the wordmark already sits next to the
   * mark, so screen readers hear the product name once rather than twice.
   */
  label?: string
}

/**
 * Five nodes, one per 72°, starting at 12 o'clock. The amber one stays at the
 * top: it is the part of the mark people recognise first, so nothing here may
 * rotate it away.
 */
const NODE_ROTATIONS = [0, 72, 144, 216, 288]

/** Warm brand constants, not theme tokens: the plate is dark in both themes. */
const PLATE = '#1e1b19'
const NODE = '#d1502a'
const STAR = '#fdf8ef'

/**
 * The AG Swarmer mark: a swarm of agents leaning in on one answer. Each node is
 * a speech bubble whose tail points at the centre star, so the shape reads as
 * "many voices, one room" at 20px as well as at 200px.
 *
 * Inline SVG rather than the packaged PNG — it stays crisp at every size and is
 * the only form the entrance animation can address per node.
 */
export function BrandMark({ className, animated = false, label }: BrandMarkProps) {
  const gradientId = `${useId()}-brand-amber`

  return (
    <svg
      viewBox="0 0 120 120"
      className={cn('shrink-0', className)}
      {...(label ? { role: 'img', 'aria-label': label } : { 'aria-hidden': true })}
    >
      <defs>
        <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#efb051" />
          <stop offset="100%" stopColor="#e08a2c" />
        </linearGradient>
      </defs>

      <rect width="120" height="120" rx="30" fill={PLATE} />
      {/* Hairline edge so the plate still separates from a dark background. */}
      <rect
        x="0.5"
        y="0.5"
        width="119"
        height="119"
        rx="29.5"
        fill="none"
        stroke={STAR}
        strokeOpacity="0.1"
      />

      <g className={animated ? 'animate-brand-ring' : undefined}>
        {NODE_ROTATIONS.map((rotation, index) => (
          <g key={rotation} transform={`rotate(${rotation} 60 60)`}>
            <g
              className={animated ? 'animate-brand-node' : undefined}
              style={animated ? { animationDelay: `${index * 110}ms` } : undefined}
              fill={index === 0 ? `url(#${gradientId})` : NODE}
            >
              <circle cx="60" cy="27" r="13.8" />
              {/* The bubble's tail. Both ends sit inside the circle so the two
                  shapes union into one silhouette. */}
              <path d="M54.5 37.5 Q59.2 42.6 60.4 45 Q64.2 41.4 66 37.5 Z" />
            </g>
          </g>
        ))}
      </g>

      <path
        className={animated ? 'animate-brand-star' : undefined}
        fill={STAR}
        d="M60 47.5 C62 54.2 65.8 58 72.5 60 C65.8 62 62 65.8 60 72.5 C58 65.8 54.2 62 47.5 60 C54.2 58 58 54.2 60 47.5 Z"
      />
    </svg>
  )
}
