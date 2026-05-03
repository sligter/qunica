import { useLocation } from 'react-router-dom'

import { AgentsList } from '@/components/layout/AgentsList'
import { GroupsList } from '@/components/layout/GroupsList'

/**
 * Decides which middle-column list to mount based on the current route.
 *
 * Not implemented as a nested `<Outlet />` because there is exactly one
 * middle-column variant per top-level section, and dispatching by pathname
 * keeps the route table flat.
 */
export function MiddleColumn() {
  const { pathname } = useLocation()
  if (pathname.startsWith('/agents')) return <AgentsList />
  return <GroupsList />
}
