import type { ReactNode } from 'react'
import { AlertTriangle, LoaderCircle, RefreshCw, Sparkles } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { useBackendStartup } from '@/hooks/useBackendStartup'

interface DesktopStartupGateProps {
  children: ReactNode
}

function formatElapsed(ms: number): string {
  return `${Math.max(1, Math.round(ms / 1_000))}s`
}

export function DesktopStartupGate({ children }: DesktopStartupGateProps) {
  const { isDesktop, ready, elapsedMs, isChecking, slow, stillWaiting, checkNow } =
    useBackendStartup()

  if (!isDesktop || ready) {
    return <>{children}</>
  }

  return (
    <div className="min-h-screen bg-background text-foreground">
      <div className="mx-auto flex min-h-screen w-full max-w-3xl flex-col justify-center px-6 py-10">
        <div className="mb-10 flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-md border border-border bg-card">
            <Sparkles className="h-5 w-5 text-primary" aria-hidden="true" />
          </div>
          <div>
            <div className="text-sm font-semibold">AG Swarmer</div>
            <div className="text-xs text-muted-foreground">Opening your workspace</div>
          </div>
        </div>

        <main className="border-t border-border pt-8" role="status" aria-live="polite">
          <div className="flex flex-col gap-6 sm:flex-row sm:items-start">
            <div className="mt-1 flex h-10 w-10 shrink-0 items-center justify-center rounded-md bg-muted">
              {stillWaiting ? (
                <AlertTriangle className="h-5 w-5 text-amber-600" aria-hidden="true" />
              ) : (
                <LoaderCircle className="h-5 w-5 animate-spin text-primary" aria-hidden="true" />
              )}
            </div>

            <div className="min-w-0 flex-1">
              <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div>
                  <h1 className="text-2xl font-semibold leading-tight">
                    {stillWaiting ? 'This is taking longer than expected' : 'Preparing your workspace'}
                  </h1>
                  <p className="mt-2 max-w-xl text-sm leading-6 text-muted-foreground">
                    {slow
                      ? 'We are getting everything ready. Your workspace will open automatically when it is ready.'
                      : 'Your workspace will open automatically in a moment.'}
                  </p>
                </div>

                <Button
                  className="w-fit shrink-0"
                  variant="outline"
                  size="sm"
                  type="button"
                  onClick={checkNow}
                  disabled={isChecking}
                >
                  <RefreshCw className={isChecking ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
                  Retry
                </Button>
              </div>

              <div className="mt-7 h-1.5 overflow-hidden rounded-full bg-muted">
                <div className="h-full w-1/3 rounded-full bg-primary animate-startup-progress" />
              </div>

              <dl className="mt-6 grid gap-3 border-t border-border pt-5 text-sm sm:grid-cols-3">
                <div>
                  <dt className="text-xs uppercase text-muted-foreground">Stage</dt>
                  <dd className="mt-1 font-medium">{isChecking ? 'Opening workspace' : 'Preparing'}</dd>
                </div>
                <div>
                  <dt className="text-xs uppercase text-muted-foreground">Time</dt>
                  <dd className="mt-1 font-medium">{formatElapsed(elapsedMs)}</dd>
                </div>
                <div>
                  <dt className="text-xs uppercase text-muted-foreground">Note</dt>
                  <dd className="mt-1 font-medium">
                    {slow ? 'First launch after an update can take a little longer.' : 'Keep this window open.'}
                  </dd>
                </div>
              </dl>
            </div>
          </div>
        </main>
      </div>
    </div>
  )
}
