import { cn } from '@/lib/utils'

/**
 * The one selected / hover / focus treatment shared by every navigable row in
 * the app: library list rows, the resource rail, settings navigation, group
 * member rows.
 *
 * Selection is encoded three ways at once — a 2px accent bar, a raised surface
 * and a heavier weight. `--color-accent` sits deliberately close to the
 * background (one step of lightness in both themes), which is what keeps a list
 * of twenty rows calm; the bar and the weight are what make the selected one
 * findable anyway, including for anyone who cannot separate those two steps.
 *
 * Layout is left to the caller: a list row is `items-start` around an avatar, a
 * navigation row is `items-center` around an icon, and forcing either here would
 * mean every caller immediately overriding it.
 */
export function navItemClass(isActive: boolean, className?: string): string {
  return cn(
    'group relative flex w-full rounded-lg text-left transition-colors',
    'focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
    isActive
      ? [
          'bg-accent font-semibold text-foreground',
          // Inset rather than full-height: a bar running the whole edge would
          // meet the row's own corner radius and read as a broken border.
          'before:absolute before:inset-y-1.5 before:left-0 before:w-0.5',
          'before:rounded-full before:bg-primary before:content-[""]',
        ]
      : 'text-foreground hover:bg-card-hover',
    className,
  )
}
