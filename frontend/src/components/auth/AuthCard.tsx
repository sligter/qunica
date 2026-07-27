import type { ReactNode } from 'react'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

interface AuthCardProps {
  /** Card heading — the page's own title. */
  title: string
  /** The form plus its switch-to-the-other-mode line. */
  children: ReactNode
}

/**
 * The signed-out surface: product wordmark over a single centered card. Login
 * and register share it so the two screens can't drift apart.
 */
export function AuthCard({ title, children }: AuthCardProps) {
  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-6">
      <div className="flex w-full max-w-sm flex-col items-center gap-6">
        <h1 className="font-serif text-2xl font-semibold tracking-tight">AG Swarmer</h1>
        <Card className="w-full shadow-md">
          <CardHeader>
            <CardTitle>{title}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">{children}</CardContent>
        </Card>
      </div>
    </div>
  )
}
