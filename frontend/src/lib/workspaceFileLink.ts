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
  /(?:[\w.-]+[/\\])*[\w.-]+\.(?:md|markdown|txt|csv|tsv|json|jsonl|ya?ml|toml|ini|cfg|conf|log|xml|html?|css|jsx?|tsx?|vue|py|sh|bash|bat|ps1|sql|rst|pdf|docx?|xlsx?|pptx?|png|jpe?g|gif|svg|webp|mp4|mp3|wav|zip|gz|tar|rs|go|java|rb|php|c|cpp|h|hpp|kt|swift)\b/gi

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

export function parseWorkspaceFileHref(
  href: string | undefined,
  root?: string,
): string | null {
  if (!href) return null
  const encoded = href.startsWith(WORKSPACE_FILE_SCHEME)
  let raw = encoded ? href.slice(WORKSPACE_FILE_SCHEME.length) : href
  try {
    raw = decodeURIComponent(raw)
  } catch {
    // Keep the original text when an agent emits a malformed percent escape.
  }
  if (!encoded && /^(?:file:|https?:\/\/tauri\.localhost\/)/i.test(raw)) {
    try {
      raw = new URL(raw).pathname.replace(/^\/(?=[a-z]:\/)/i, '')
    } catch {
      return null
    }
  }
  raw = raw
    .replace(/#L\d+(?:C\d+)?$/i, '')
    .replace(/:\d+(?::\d+)?$/, '')
    .replace(/\\/g, '/')

  const driveAbsolute = /^[a-z]:\//i.test(raw)
  const posixAbsolute = raw.startsWith('/') && !raw.startsWith('//')
  if (encoded && (driveAbsolute || posixAbsolute)) return null
  if (!encoded && /^[a-z][a-z0-9+.-]*:/i.test(raw) && !driveAbsolute) return null
  if (!encoded && !driveAbsolute && !posixAbsolute && !/[^/]\.[^/]+$/.test(raw)) return null

  if (!encoded && (driveAbsolute || posixAbsolute)) {
    if (!root) return null
    const normalizedRoot = root.replace(/\\/g, '/').replace(/^\/\/\?\//, '').replace(/\/+$/, '')
    const foldedRoot = driveAbsolute ? normalizedRoot.toLowerCase() : normalizedRoot
    const foldedRaw = driveAbsolute ? raw.toLowerCase() : raw
    if (!foldedRaw.startsWith(`${foldedRoot}/`)) return null
    raw = raw.slice(normalizedRoot.length + 1)
  }

  const rel = normalizeRel(raw).replace(/^\/+/, '')
  if (!rel || rel.split('/').includes('..') || rel.startsWith('#') || rel.startsWith('?')) {
    return null
  }
  return rel
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
