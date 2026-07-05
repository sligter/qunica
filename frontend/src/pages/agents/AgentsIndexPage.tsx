import { Bot } from 'lucide-react'

export function AgentsIndexPage() {
  return (
    <div className="flex h-full w-full flex-col items-center justify-center gap-3 bg-background p-6 text-center">
      <div className="flex h-14 w-14 items-center justify-center rounded-full bg-muted text-muted-foreground">
        <Bot className="h-7 w-7" />
      </div>
      <h2 className="text-base font-medium">Select an agent</h2>
      <p className="max-w-sm text-sm text-muted-foreground">
        Pick an agent from the left, or click <span className="font-medium">+</span> in
        the column header to create a new one.
      </p>
    </div>
  )
}
