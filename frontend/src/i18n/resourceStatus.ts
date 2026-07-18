const KNOWN_RESOURCE_STATUSES = new Set([
  'active',
  'inactive',
  'deleted',
  'archived',
  'disabled',
  'draft',
  'pending',
])

export function formatResourceStatus(
  status: string,
  translate: (key: string, options?: { status: string }) => string,
): string {
  return KNOWN_RESOURCE_STATUSES.has(status)
    ? translate(`common:status.${status}`)
    : translate('common:status.unknown', { status })
}
