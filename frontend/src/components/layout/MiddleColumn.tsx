import { useLocation } from 'react-router-dom'

import { GroupsList } from '@/components/layout/GroupsList'

/**
 * Middle column shows the groups list for group routes only.
 * Agents, Providers, and Skills use full-page card layouts with dialogs.
 */
export function MiddleColumn() {
  const { pathname } = useLocation()
  if (pathname.startsWith('/groups')) return <GroupsList />
  return null
}
