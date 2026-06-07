import { useLocation } from 'react-router-dom'

import { GroupsList } from '@/components/layout/GroupsList'
import { VerticalResizeHandle } from '@/components/layout/VerticalResizeHandle'
import { usePersistentPaneWidth } from '@/hooks/usePersistentPaneWidth'

/**
 * Middle column shows the groups list for group routes only.
 * Agents, Providers, and Skills use full-page card layouts with dialogs.
 */
export function MiddleColumn() {
  const { pathname } = useLocation()
  if (pathname.startsWith('/groups')) return <GroupsMiddleColumn />
  return null
}

function GroupsMiddleColumn() {
  const groupsPane = usePersistentPaneWidth({
    storageKey: 'ag-swarmer:layout:groups-pane-width',
    defaultWidth: 288,
    minWidth: 224,
    maxWidth: 420,
  })

  return (
    <div className="flex h-full shrink-0">
      <GroupsList width={groupsPane.width} />
      <VerticalResizeHandle
        label="Resize groups column"
        value={groupsPane.width}
        min={groupsPane.minWidth}
        max={groupsPane.maxWidth}
        onResizeStart={(event) => groupsPane.startResize(event)}
        onStep={groupsPane.resizeBy}
      />
    </div>
  )
}
