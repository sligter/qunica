import { MessagesSquare } from 'lucide-react'

export function GroupsRightPane() {
  return (
    <div className="flex h-full w-full flex-col items-center justify-center gap-3 bg-background p-6 text-center">
      <div className="flex h-14 w-14 items-center justify-center rounded-full bg-muted text-muted-foreground">
        <MessagesSquare className="h-7 w-7" />
      </div>
      <h2 className="text-base font-medium">Select a group</h2>
      <p className="max-w-sm text-sm text-muted-foreground">
        Pick a conversation from the left, or click <span className="font-medium">+</span>{' '}
        in the column header to start a new group.
      </p>
    </div>
  )
}
