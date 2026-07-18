import { useEffect, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Outlet, useNavigate } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'

import { Button } from '@/components/ui/button'

interface EntityLayoutProps {
  /** Navigation resource key for the area heading. */
  titleKey: 'agents' | 'providers' | 'skills' | 'workspaces'
  /** The searchable entity list column rendered left of the detail Outlet. */
  list: ReactNode
}

/**
 * Standalone shell for a top-level entity area (Agents, Providers, Skills,
 * Workspaces): a header row with a back-to-chat button plus the serif area
 * title, and below it the entity list column next to the detail/create Outlet.
 */
export function EntityLayout({ titleKey, list }: EntityLayoutProps) {
  const { t } = useTranslation('navigation')
  const navigate = useNavigate()
  const title = t(titleKey)

  useEffect(() => {
    document.title = title
  }, [title])

  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-background">
      <div className="flex h-14 shrink-0 items-center gap-2 border-b border-border px-3">
        <Button
          variant="ghost"
          size="icon"
          onClick={() => void navigate('/')}
          aria-label={t('backToChat')}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <h1 className="font-serif text-base font-semibold tracking-tight">{title}</h1>
      </div>
      <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden">
        {list}
        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <Outlet />
        </div>
      </div>
    </div>
  )
}
