import { useState } from 'react'
import { Check, GitBranch, Link, Pencil, Plus, Trash2 } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import {
  useCreateGroupWorkspaceGitBranch,
  useDeleteGroupWorkspaceGitBranch,
  useGroupWorkspaceGitBranches,
  useRenameGroupWorkspaceGitBranch,
  useSwitchGroupWorkspaceGitBranch,
} from '@/hooks/useWorkspaceGit'
import type { GroupWorkspaceGitBranch } from '@/types/api'

interface WorkspaceGitBranchSheetProps {
  groupId: string | undefined
  open: boolean
  onOpenChange: (open: boolean) => void
  onError: (error: string | null) => void
  onSetRemote: () => void
}

function displayError(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

export function WorkspaceGitBranchSheet({
  groupId,
  open,
  onOpenChange,
  onError,
  onSetRemote,
}: WorkspaceGitBranchSheetProps) {
  const [newBranch, setNewBranch] = useState('')
  const [renameBranch, setRenameBranch] = useState<GroupWorkspaceGitBranch | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const [deleteBranch, setDeleteBranch] = useState<GroupWorkspaceGitBranch | null>(null)
  const branches = useGroupWorkspaceGitBranches(groupId)
  const create = useCreateGroupWorkspaceGitBranch(groupId)
  const switchBranch = useSwitchGroupWorkspaceGitBranch(groupId)
  const rename = useRenameGroupWorkspaceGitBranch(groupId)
  const remove = useDeleteGroupWorkspaceGitBranch(groupId)
  const local = branches.data?.branches.filter((branch) => branch.kind === 'local') ?? []
  const remote = branches.data?.branches.filter((branch) => branch.kind === 'remote') ?? []

  const run = (operation: () => Promise<unknown>) => {
    onError(null)
    void operation().catch((error: unknown) => onError(displayError(error)))
  }

  const branchRow = (branch: GroupWorkspaceGitBranch) => <div key={`${branch.kind}:${branch.full_name}`} className="group flex min-w-0 items-center gap-2 px-4 py-2 hover:bg-muted/70"><button type="button" className="flex min-w-0 flex-1 items-center gap-2 text-left" disabled={branch.current || switchBranch.isPending} onClick={() => run(() => switchBranch.mutateAsync({ name: branch.name, kind: branch.kind }))}><GitBranch className="h-3.5 w-3.5 shrink-0 text-muted-foreground" /><span className="truncate text-xs">{branch.name}</span>{branch.current ? <Check className="ml-auto h-3.5 w-3.5 text-primary" /> : null}</button>{branch.kind === 'local' && !branch.current ? <><Button type="button" variant="ghost" size="icon" className="h-7 w-7 opacity-0 group-hover:opacity-100 focus-visible:opacity-100" onClick={() => { setRenameBranch(branch); setRenameValue(branch.name) }} aria-label={`Rename ${branch.name}`} title="Rename branch"><Pencil className="h-3.5 w-3.5" /></Button><Button type="button" variant="ghost" size="icon" className="h-7 w-7 opacity-0 group-hover:opacity-100 focus-visible:opacity-100" onClick={() => setDeleteBranch(branch)} aria-label={`Delete ${branch.name}`} title="Delete branch"><Trash2 className="h-3.5 w-3.5 text-destructive" /></Button></> : null}</div>

  return <>
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent className="w-[min(100vw,30rem)]">
        <SheetHeader className="shrink-0 border-b border-border px-5 py-4 pr-14">
          <SheetTitle>Branches</SheetTitle>
          <SheetDescription>Switch and manage local or remote branches.</SheetDescription>
        </SheetHeader>
        <form className="flex gap-2 border-b border-border p-4" onSubmit={(event) => { event.preventDefault(); const name = newBranch.trim(); if (name) run(() => create.mutateAsync({ name }).then(() => setNewBranch(''))) }}>
          <Input value={newBranch} onChange={(event) => setNewBranch(event.target.value)} placeholder="New branch name" className="h-8 text-xs" aria-label="New branch name" />
          <Button type="submit" size="icon" className="h-8 w-8 shrink-0" disabled={!newBranch.trim() || create.isPending} aria-label="Create branch" title="Create branch"><Plus className="h-3.5 w-3.5" /></Button>
        </form>
        <div className="min-h-0 flex-1 overflow-y-auto">
          <p className="px-4 py-2 text-[11px] font-medium uppercase text-muted-foreground">Local</p>
          {local.map(branchRow)}
          <div className="flex items-center justify-between border-t border-border px-4 py-2">
            <p className="text-[11px] font-medium uppercase text-muted-foreground">Remote</p>
            <Button type="button" variant="ghost" size="sm" className="h-6 gap-1 px-1.5 text-[11px]" onClick={onSetRemote} title="Set remote URL"><Link className="h-3 w-3" /> Remote URL</Button>
          </div>
          {remote.length ? remote.map(branchRow) : <p className="px-4 py-2 text-xs text-muted-foreground">No remote branches.</p>}
        </div>
      </SheetContent>
    </Sheet>
    <Sheet open={renameBranch !== null} onOpenChange={(next) => { if (!next) setRenameBranch(null) }}>
      <SheetContent className="w-[min(100vw,26rem)]">
        <SheetHeader className="border-b border-border px-5 py-4 pr-14"><SheetTitle>Rename branch</SheetTitle></SheetHeader>
        <div className="space-y-3 p-5"><Input value={renameValue} onChange={(event) => setRenameValue(event.target.value)} aria-label="New branch name" /><Button type="button" className="w-full" disabled={!renameValue.trim() || rename.isPending} onClick={() => run(async () => { if (renameBranch) await rename.mutateAsync({ old: renameBranch.name, new: renameValue.trim() }); setRenameBranch(null) })}>Rename branch</Button></div>
      </SheetContent>
    </Sheet>
    <ConfirmDialog open={deleteBranch !== null} onOpenChange={(next) => { if (!next) setDeleteBranch(null) }} title="Delete branch?" description={deleteBranch ? `Delete ${deleteBranch.name}? This cannot be undone.` : undefined} confirmLabel="Delete branch" destructive onConfirm={async () => { if (deleteBranch) await remove.mutateAsync({ name: deleteBranch.name, force: false }) }} />
  </>
}
