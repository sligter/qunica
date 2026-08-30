export interface MentionPart {
  text: string
  mentioned: boolean
}

export const MENTION_CLASS_NAME = 'chat-mention'

interface MdastNode {
  type: string
  value?: string
  children?: MdastNode[]
  data?: {
    hName?: string
    hProperties?: Record<string, unknown>
  }
}

function mentionPattern(names: readonly string[]): RegExp | null {
  const alternatives = [...new Set(names.filter(Boolean))]
    .sort((a, b) => b.length - a.length)
    .map((name) => name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
  return alternatives.length > 0
    ? new RegExp(`@(?:${alternatives.join('|')})(?![\\p{L}\\p{N}_-])`, 'giu')
    : null
}

function splitWithPattern(text: string, pattern: RegExp | null): MentionPart[] {
  if (!pattern || !text.includes('@')) return [{ text, mentioned: false }]

  const parts: MentionPart[] = []
  let lastIndex = 0
  pattern.lastIndex = 0
  for (const match of text.matchAll(pattern)) {
    const start = match.index
    if (start > lastIndex) parts.push({ text: text.slice(lastIndex, start), mentioned: false })
    parts.push({ text: match[0], mentioned: true })
    lastIndex = start + match[0].length
  }
  if (lastIndex < text.length) parts.push({ text: text.slice(lastIndex), mentioned: false })
  return parts.length > 0 ? parts : [{ text, mentioned: false }]
}

export function splitMentions(text: string, names: readonly string[]): MentionPart[] {
  return splitWithPattern(text, mentionPattern(names))
}

const SKIP_PARENTS = new Set(['link', 'linkReference', 'inlineCode', 'code'])

function walk(node: MdastNode, pattern: RegExp): void {
  if (!node.children) return
  const children: MdastNode[] = []
  for (const child of node.children) {
    if (child.type === 'text' && !SKIP_PARENTS.has(node.type)) {
      children.push(...splitWithPattern(child.value ?? '', pattern).map((part) => (
        part.mentioned
          ? {
              type: 'text',
              value: part.text,
              data: {
                hName: 'span',
                hProperties: { className: MENTION_CLASS_NAME.split(' ') },
              },
            }
          : { type: 'text', value: part.text }
      )))
    } else {
      walk(child, pattern)
      children.push(child)
    }
  }
  node.children = children
}

export function createRemarkMentions(names: readonly string[]) {
  const pattern = mentionPattern(names)
  return function remarkMentions() {
    return (tree: unknown): void => {
      if (pattern) walk(tree as MdastNode, pattern)
    }
  }
}
