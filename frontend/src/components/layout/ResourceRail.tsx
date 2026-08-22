import { useTranslation } from 'react-i18next'
import { BarChart3, Bot, Folder, Plug, Server, Sparkles } from 'lucide-react'

import { OverlayNavLink } from '@/components/layout/overlayRouting'
import { navItemClass } from '@/lib/navItemClass'

interface RailItem {
  to: string
  /** Key in the `navigation` namespace. */
  key: string
  icon: typeof Bot
}

interface RailGroup {
  /** Key under `navigation.rail`. */
  key: 'resources' | 'insights'
  items: RailItem[]
}

/**
 * Two groups rather than one flat list: the first five are things you create
 * and edit, the last one is a read-only report. Sorting a report in among them
 * is what made "Token usage" feel like a sixth kind of resource.
 */
const RAIL_GROUPS: RailGroup[] = [
  {
    key: 'resources',
    items: [
      { to: '/agents', key: 'agents', icon: Bot },
      { to: '/providers', key: 'providers', icon: Plug },
      { to: '/mcp-servers', key: 'mcpServers', icon: Server },
      { to: '/skills', key: 'skills', icon: Sparkles },
      { to: '/workspaces', key: 'workspaces', icon: Folder },
    ],
  },
  {
    key: 'insights',
    items: [{ to: '/usage', key: 'usage', icon: BarChart3 }],
  },
]

/**
 * Persistent second-level navigation for the library areas.
 *
 * Before this, moving from Agents to MCP servers meant closing the panel,
 * reopening the sidebar flyout and picking again — three steps to cross between
 * two screens that look identical. The rail makes it one.
 *
 * Three shapes, all in CSS so there is no layout flash and no media-query hook
 * to keep in sync with the breakpoints:
 *
 * - `< lg` — a horizontal scroller above the list, labels shown
 * - `lg`   — a 56px icon strip, labels dropped (kept as `title` for the pointer)
 * - `xl`   — 200px, labels and group headings shown
 */
export function ResourceRail() {
  const { t } = useTranslation('navigation')

  return (
    <nav
      aria-label={t('library')}
      className={[
        'flex shrink-0 gap-1 overflow-auto overscroll-contain border-border bg-card/60',
        'max-lg:w-full max-lg:flex-row max-lg:items-center max-lg:border-b max-lg:px-2 max-lg:py-1.5',
        'lg:h-full lg:w-14 lg:flex-col lg:border-r lg:p-2',
        'xl:w-[200px] xl:p-3',
      ].join(' ')}
      onKeyDown={onRailKeys}
    >
      {RAIL_GROUPS.map((group, index) => (
        <div
          key={group.key}
          className={[
            'flex gap-1',
            'max-lg:flex-row max-lg:items-center',
            'lg:flex-col',
            // The groups read as one strip when they are side by side, so the
            // break between them is a rule rather than a heading down there.
            index > 0
              ? 'max-lg:ml-1 max-lg:border-l max-lg:border-border max-lg:pl-2 lg:mt-2 lg:border-t lg:border-border lg:pt-2'
              : '',
          ].join(' ')}
        >
          <p className="hidden px-2.5 pb-1 text-xs font-medium uppercase tracking-wider text-muted-foreground xl:block">
            {t(`rail.${group.key}`)}
          </p>
          {group.items.map(({ to, key, icon: Icon }) => {
            const label = t(key)
            return (
              <OverlayNavLink
                key={to}
                to={to}
                title={label}
                data-rail-item=""
                className={({ isActive }) =>
                  navItemClass(
                    isActive,
                    [
                      'items-center gap-2.5 px-2.5 py-1.5 text-sm',
                      'max-lg:w-auto max-lg:shrink-0',
                      'lg:justify-center lg:px-0',
                      'xl:justify-start xl:px-2.5',
                    ].join(' '),
                  )
                }
              >
                <Icon className="h-4 w-4 shrink-0 text-muted-foreground" />
                <span className="truncate lg:hidden xl:inline">{label}</span>
              </OverlayNavLink>
            )
          })}
        </div>
      ))}
    </nav>
  )
}

/**
 * Arrow-key roving across the whole rail, groups included. Matches the sidebar
 * flyout's keys so the two navigations behave the same.
 */
function onRailKeys(event: React.KeyboardEvent<HTMLElement>) {
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
  event.preventDefault()
  const items = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('[data-rail-item]'))
  if (items.length === 0) return
  const current = items.indexOf(document.activeElement as HTMLElement)
  const next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? items.length - 1
        : event.key === 'ArrowDown'
          ? (current + 1) % items.length
          : (current - 1 + items.length) % items.length
  items[next]?.focus()
}
