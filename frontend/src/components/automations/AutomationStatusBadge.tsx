import { useTranslation } from 'react-i18next'

import { Badge, type BadgeProps } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
import type { AutomationRunStatus } from '@/types/api'

function variantForStatus(status: AutomationRunStatus): BadgeProps['variant'] {
  switch (status) {
    case 'succeeded':
      return 'success'
    case 'failed':
    case 'interrupted':
      return 'destructive'
    case 'running':
    case 'queued':
      return 'info'
    case 'waiting_for_user':
      return 'warning'
    default:
      return 'secondary'
  }
}

export function AutomationStatusBadge({
  status,
  className,
}: {
  status: AutomationRunStatus
  className?: string
}) {
  const { t } = useTranslation('automations')
  return (
    <Badge variant={variantForStatus(status)} className={cn('font-medium', className)}>
      {t(`status.${status}`)}
    </Badge>
  )
}
