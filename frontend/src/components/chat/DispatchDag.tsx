import { AlertTriangle, GitBranch, Link2Off, Route } from 'lucide-react'

import { cn } from '@/lib/utils'
import type { AgentDispatchTrace, PublicTurnArtifact } from '@/lib/api-v2/types'

interface DispatchDagProps {
  dispatches: readonly AgentDispatchTrace[]
  className?: string
}

type EdgeIssue = 'orphan' | 'cycle'

interface DagNode {
  dispatch: AgentDispatchTrace
  children: DagNode[]
  issue?: EdgeIssue
}

function edgeIssue(
  dispatch: AgentDispatchTrace,
  dispatchById: ReadonlyMap<string, AgentDispatchTrace>,
): EdgeIssue | undefined {
  const parentId = dispatch.parent_dispatch_id
  if (!parentId) return undefined
  if (!dispatchById.has(parentId)) return 'orphan'

  const visited = new Set<string>([dispatch.id])
  let currentId: string | null = parentId
  while (currentId) {
    if (visited.has(currentId)) return 'cycle'
    visited.add(currentId)
    currentId = dispatchById.get(currentId)?.parent_dispatch_id ?? null
  }
  return undefined
}

function buildDispatchForest(dispatches: readonly AgentDispatchTrace[]): DagNode[] {
  const dispatchById = new Map<string, AgentDispatchTrace>()
  for (const dispatch of dispatches) {
    if (!dispatchById.has(dispatch.id)) dispatchById.set(dispatch.id, dispatch)
  }

  const nodes = new Map<string, DagNode>()
  for (const dispatch of dispatchById.values()) {
    nodes.set(dispatch.id, { dispatch, children: [], issue: edgeIssue(dispatch, dispatchById) })
  }

  const roots: DagNode[] = []
  for (const node of nodes.values()) {
    const parentId = node.dispatch.parent_dispatch_id
    const parent = parentId ? nodes.get(parentId) : undefined
    if (!parent || node.issue) {
      roots.push(node)
    } else {
      parent.children.push(node)
    }
  }
  return roots
}

function humanize(value: string): string {
  return value.replace(/_/g, ' ')
}

function ArtifactDetails({ artifact }: { artifact: PublicTurnArtifact | null }) {
  if (!artifact) return null
  return (
    <dl className="mt-2 grid grid-cols-[auto_minmax(0,1fr)] gap-x-2 gap-y-1 border-t border-border pt-2 text-[11px]">
      {artifact.mode ? <><dt className="text-muted-foreground">Mode</dt><dd>{artifact.mode}</dd></> : null}
      {artifact.target_agent_id ? <><dt className="text-muted-foreground">Target</dt><dd className="truncate" title={artifact.target_agent_id}>{artifact.target_agent_id}</dd></> : null}
      {artifact.child_dispatch_id ? <><dt className="text-muted-foreground">Child</dt><dd className="truncate" title={artifact.child_dispatch_id}>{artifact.child_dispatch_id}</dd></> : null}
      {artifact.outcome ? <><dt className="text-muted-foreground">Outcome</dt><dd className="break-words">{artifact.outcome}</dd></> : null}
      {artifact.failure_code ? <><dt className="text-muted-foreground">Failure</dt><dd className="break-words text-destructive">{artifact.failure_code}</dd></> : null}
    </dl>
  )
}

function DagBranch({ node, depth = 0 }: { node: DagNode; depth?: number }) {
  const IssueIcon = node.issue === 'orphan' ? Link2Off : AlertTriangle
  const dispatch = node.dispatch
  return (
    <li className="relative min-w-0">
      {depth > 0 ? <span className="absolute -left-4 top-4 h-px w-4 bg-border" aria-hidden="true" /> : null}
      <div className="min-w-0 rounded-md border border-border bg-background px-3 py-2">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <Route className="h-3.5 w-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
          <span className="min-w-0 truncate font-mono text-xs font-semibold" title={dispatch.target_agent_id}>
            {dispatch.target_agent_id}
          </span>
          <span className="rounded-[3px] bg-muted px-1.5 py-0.5 text-[10px] capitalize text-muted-foreground">
            {humanize(dispatch.action_kind)}
          </span>
          <span className={cn('ml-auto text-[10px] capitalize text-muted-foreground', dispatch.status === 'failed' && 'text-destructive')}>
            {humanize(dispatch.status)}
          </span>
        </div>
        <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
          <span>{humanize(dispatch.selection_reason)}</span>
          <span>hop {dispatch.hop}</span>
          <span>{dispatch.total_tokens.toLocaleString()} tokens</span>
          {node.issue ? (
            <span className="inline-flex items-center gap-1 text-warning-foreground" data-edge-issue={node.issue}>
              <IssueIcon className="h-3 w-3" aria-hidden="true" />
              {node.issue === 'orphan' ? 'Missing parent' : 'Cycle detached'}
            </span>
          ) : null}
        </div>
        {dispatch.failure_code ? <p className="mt-1 break-words text-[11px] text-destructive">{dispatch.failure_code}</p> : null}
        <ArtifactDetails artifact={dispatch.artifact} />
      </div>
      {node.children.length > 0 ? (
        <ul className="ml-4 space-y-2 border-l border-border pl-4 pt-2">
          {node.children.map((child) => <DagBranch key={child.dispatch.id} node={child} depth={depth + 1} />)}
        </ul>
      ) : null}
    </li>
  )
}

export function DispatchDag({ dispatches, className }: DispatchDagProps) {
  const forest = buildDispatchForest(dispatches)
  if (forest.length === 0) {
    return <p className={cn('py-6 text-center text-sm text-muted-foreground', className)}>No dispatches recorded.</p>
  }
  return (
    <div className={cn('min-w-0', className)}>
      <div className="mb-2 flex items-center gap-2 text-xs font-medium text-muted-foreground">
        <GitBranch className="h-3.5 w-3.5" aria-hidden="true" />
        Dispatch path
      </div>
      <ul className="space-y-2" aria-label="Dispatch path">
        {forest.map((node) => <DagBranch key={node.dispatch.id} node={node} />)}
      </ul>
    </div>
  )
}
