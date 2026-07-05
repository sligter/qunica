import { Outlet, useNavigate } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'

import { Button } from '@/components/ui/button'

/**
 * Minimal settings shell: a header row with a back-to-chat button plus the
 * serif "Settings" title, and the global settings content below. Entity areas
 * (Agents, Providers, ...) live at top-level routes with their own EntityLayout.
 */
export function SettingsLayout() {
  const navigate = useNavigate()

  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-background">
      <div className="flex h-14 shrink-0 items-center gap-2 border-b border-border px-3">
        <Button
          variant="ghost"
          size="icon"
          onClick={() => void navigate('/')}
          aria-label="Back to chat"
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <h1 className="font-serif text-base font-semibold tracking-tight">Settings</h1>
      </div>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        <Outlet />
      </div>
    </div>
  )
}
