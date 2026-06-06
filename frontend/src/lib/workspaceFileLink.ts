/**
 * Detects workspace file references in chat markdown and turns them into
 * clickable links that open the file in the right-hand workspace panel.
 *
 * Matched paths are encoded as `workspace-file:<rel>` hrefs (relative posix
 * path). `MarkdownMessage` recognises that scheme, opens the panel preview on
 * click, and shows the absolute path on hover.
 */

export const WORKSPACE_FILE_SCHEME = 'workspace-file:'

// Filenames / relative paths ending in a known file extension. Optional leading
// directory segments use either posix or windows separators.
const FILE_PATH_RE =
  /(?:[\w.\-]+[/\\])*[\w.\-]+\.(?:md|markdown|txt|csv|tsv|json|jsonl|ya?ml|toml|ini|cfg|conf|log|xml|html?|css|jsx?|tsx?|vue|py|sh|bash|bat|ps1|sql|rst|pdf|docx?|xlsx?|pptx?|png|jpe?g|gif|svg|webp|mp4|mp3|wav|zip|gz|tar|rs|go|java|rb|php|c|cpp|h|hpp|kt|swift)\b/gi

interface MdastNode {
  type: string
  value?: string
  url?: string
  children?: MdastNode[]
}

function normalizeRel(raw: string): string {
  return raw.replace(/\\/g, '/').replace(/^\.\//, '')
}

function splitText(value: string): MdastNode[] {
  const nodes: MdastNode[] = []
  let lastIndex = 0
  let pushed = false
  FILE_PATH_RE.lastIndex = 0
  let match: RegExpExecArray | null
  while ((match = FILE_PATH_RE.exec(value)) !== null) {
    const matchText = match[0]
    const start = match.index
    // Skip paths that are part of a URL (e.g. https://host/page.html).
    if (start >= 3 && value.slice(start - 3, start) === '://') continue
    if (start > lastIndex) nodes.push({ type: 'text', value: value.slice(lastIndex, start) })
    nodes.push({
      type: 'link',
      url: WORKSPACE_FILE_SCHEME + encodeURIComponent(normalizeRel(matchText)),
      children: [{ type: 'text', value: matchText }],
    })
    lastIndex = start + matchText.length
    pushed = true
  }
  if (!pushed) return [{ type: 'text', value }]
  if (lastIndex < value.length) nodes.push({ type: 'text', value: value.slice(lastIndex) })
  return nodes
}

const SKIP_PARENTS = new Set(['link', 'linkReference', 'inlineCode', 'code'])

function walk(node: MdastNode): void {
  if (!node.children) return
  const out: MdastNode[] = []
  for (const child of node.children) {
    if (child.type === 'text' && !SKIP_PARENTS.has(node.type)) {
      out.push(...splitText(child.value ?? ''))
    } else {
      walk(child)
      out.push(child)
    }
  }
  node.children = out
}

export function remarkWorkspaceFiles() {
  return (tree: unknown): void => {
    walk(tree as MdastNode)
  }
}

export function parseWorkspaceFileHref(href: string | undefined): string | null {
  if (!href || !href.startsWith(WORKSPACE_FILE_SCHEME)) return null
  const raw = href.slice(WORKSPACE_FILE_SCHEME.length)
  try {
    return decodeURIComponent(raw)
  } catch {
    return raw
  }
}

export function joinWorkspaceAbsPath(
  root: string | undefined,
  separator: string | undefined,
  rel: string,
): string {
  const normalizedRel = normalizeRel(rel)
  if (!root) return normalizedRel
  const sep = separator || '/'
  const cleanRoot = root.replace(/[/\\]+$/, '')
  const cleanRel = normalizedRel.split('/').filter(Boolean).join(sep)
  return cleanRel ? `${cleanRoot}${sep}${cleanRel}` : cleanRoot
}
