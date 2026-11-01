/**
 * The bubbles an agent reply renders as.
 *
 * `response_segments` split a reply where tool calls interrupted it, but they
 * can disagree with `content`: a resumed turn appends to the content of the
 * message it continues, and a turn that stopped on a provider error keeps that
 * text in the content only. Text outside the segments gets a bubble of its own
 * rather than collapsing the whole reply into one — otherwise pressing continue
 * visibly merges the old reply and the new one.
 */
export function replySegments(
  content: string,
  segments: readonly string[] | null | undefined,
): string[] {
  const parts = (segments ?? []).filter((segment) => segment.length > 0)
  if (parts.length === 0) return [content]
  const joined = parts.join('')
  if (joined === content) return parts
  if (content.startsWith(joined)) return [...parts, content.slice(joined.length)]
  if (content.endsWith(joined)) return [content.slice(0, content.length - joined.length), ...parts]
  return [content]
}
