import type { MessageAttachment } from '@/types/api'

/**
 * A message as text, with its uploaded files named under it.
 *
 * A clipboard holds bytes for at most one image flavour, so the list is what
 * carries the rest — and it is all a non-image attachment can ever be on a
 * clipboard. Paths as well as names, so text pasted back into a composer still
 * points at the files.
 */
export function messageCopyText(
  content: string,
  attachments: readonly MessageAttachment[],
  attachmentsLabel: string,
): string {
  if (attachments.length === 0) return content
  const lines = attachments.map((attachment) => `- ${attachment.name} (${attachment.path})`)
  return `${content}\n\n${attachmentsLabel}\n${lines.join('\n')}`.trim()
}
