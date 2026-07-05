import { useLocation } from 'react-router-dom'

import { AgentsListColumn } from '@/components/layout/AgentsListColumn'
import { GroupsList } from '@/components/layout/GroupsList'
import { ProvidersListColumn } from '@/components/layout/ProvidersListColumn'
import { SkillsListColumn } from '@/components/layout/SkillsListColumn'
import { VerticalResizeHandle } from '@/components/layout/VerticalResizeHandle'
import { WorkspacesListColumn } from '@/components/layout/WorkspacesListColumn'
import { usePersistentPaneWidth } from '@/hooks/usePersistentPaneWidth'

/**
 * Middle column renders the list for the current route area:
 * groups, agents, providers, skills, and workspaces. Settings and other
 * routes have no middle column. All areas share the same persisted width.
 */
export function MiddleColumn() {
  const { pathname } = useLocation()

  if (pathname.startsWith('/groups')) {
    return (
      <ResizableColumn label="Resize groups column">
        {(width) => <GroupsList width={width} />}
      </ResizableColumn>
    )
  }
  if (pathname.startsWith('/agents')) {
    return (
      <ResizableColumn label="Resize agents column">
        {(width) => <AgentsListColumn width={width} />}
      </ResizableColumn>
    )
  }
  if (pathname.startsWith('/providers')) {
    return (
      <ResizableColumn label="Resize providers column">
        {(width) => <ProvidersListColumn width={width} />}
      </ResizableColumn>
    )
  }
  if (pathname.startsWith('/skills')) {
    return (
      <ResizableColumn label="Resize skills column">
        {(width) => <SkillsListColumn width={width} />}
      </ResizableColumn>
    )
  }
  if (pathname.startsWith('/workspaces')) {
    return (
      <ResizableColumn label="Resize workspaces column">
        {(width) => <WorkspacesListColumn width={width} />}
      </ResizableColumn>
    )
  }
  return null
}

interface ResizableColumnProps {
  label: string
  children: (width: number) => React.ReactNode
}

function ResizableColumn({ label, children }: ResizableColumnProps) {
  const pane = usePersistentPaneWidth({
    // Historical key: all middle-column lists share one persisted width.
    storageKey: 'ag-swarmer:layout:groups-pane-width',
    defaultWidth: 288,
    minWidth: 224,
    maxWidth: 420,
  })

  return (
    <div className="flex h-full shrink-0">
      {children(pane.width)}
      <VerticalResizeHandle
        label={label}
        value={pane.width}
        min={pane.minWidth}
        max={pane.maxWidth}
        onResizeStart={(event) => pane.startResize(event)}
        onStep={pane.resizeBy}
      />
    </div>
  )
}
