import { useState } from 'react'
import { NotebookPen, Plus, Trash2 } from 'lucide-react'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import {
  useCreateGroupNote,
  useDeleteGroupNote,
  useGroupNotes,
  useUpdateGroupNote,
} from '@/hooks/useGroupNotes'
import type { GroupNoteRead } from '@/types/api'

interface GroupNotesPanelProps {
  groupId: string
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function GroupNotesPanel({ groupId, open, onOpenChange }: GroupNotesPanelProps) {
  const notes = useGroupNotes(groupId)
  const create = useCreateGroupNote(groupId)
  const update = useUpdateGroupNote(groupId)
  const del = useDeleteGroupNote(groupId)
  const [editing, setEditing] = useState<GroupNoteRead | null>(null)
  const [creating, setCreating] = useState(false)
  const [title, setTitle] = useState('')
  const [content, setContent] = useState('')

  const openCreate = () => {
    setEditing(null)
    setCreating(true)
    setTitle('')
    setContent('')
  }

  const openEdit = (note: GroupNoteRead) => {
    setCreating(false)
    setEditing(note)
    setTitle(note.title)
    setContent(note.content)
  }

  const onSave = async () => {
    if (creating) {
      await create.mutateAsync({ title, content })
    } else if (editing) {
      await update.mutateAsync({
        noteId: editing.id,
        data: { title, content },
      })
    }
    setCreating(false)
    setEditing(null)
  }

  const isForm = creating || editing !== null
  const isPending = create.isPending || update.isPending

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <div className="flex items-center justify-between pr-8">
            <DialogTitle>Group Notes</DialogTitle>
            {!isForm && (
              <Button size="sm" variant="outline" onClick={openCreate}>
                <Plus className="mr-1 h-3.5 w-3.5" />
                New Note
              </Button>
            )}
          </div>
        </DialogHeader>

        {isForm ? (
          <div className="space-y-3">
            <div className="space-y-1.5">
              <Label htmlFor="note-title">Title</Label>
              <Input
                id="note-title"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder="Note title"
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="note-content">Content</Label>
              <Textarea
                id="note-content"
                rows={10}
                value={content}
                onChange={(e) => setContent(e.target.value)}
                placeholder="Write your note…"
              />
            </div>
            <DialogFooter>
              <Button
                variant="outline"
                onClick={() => {
                  setCreating(false)
                  setEditing(null)
                }}
              >
                Cancel
              </Button>
              <Button onClick={onSave} disabled={isPending || !title.trim()}>
                {isPending ? 'Saving…' : 'Save'}
              </Button>
            </DialogFooter>
          </div>
        ) : (
          <>
            {notes.isLoading && (
              <p className="text-sm text-muted-foreground">Loading notes…</p>
            )}

            {notes.data && notes.data.length === 0 && (
              <div className="flex flex-col items-center gap-2 py-10 text-center">
                <NotebookPen className="h-8 w-8 text-muted-foreground" />
                <p className="text-sm text-muted-foreground">No notes yet</p>
              </div>
            )}

            {notes.data && notes.data.length > 0 && (
              <ul className="divide-y divide-border">
                {notes.data.map((n) => (
                  <li key={n.id} className="flex items-center justify-between gap-3 py-2.5">
                    <button
                      type="button"
                      className="flex min-w-0 flex-1 flex-col gap-0.5 text-left hover:opacity-80"
                      onClick={() => openEdit(n)}
                    >
                      <p className="truncate text-sm font-medium">{n.title}</p>
                      <p className="line-clamp-1 text-[10px] text-muted-foreground">
                        {n.content.slice(0, 80) || 'Empty note'}
                      </p>
                    </button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 shrink-0 text-muted-foreground hover:text-red-600"
                      onClick={() => del.mutate(n.id)}
                      disabled={del.isPending}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}
