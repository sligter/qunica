import ReactMarkdown, { defaultUrlTransform } from 'react-markdown'
import rehypeKatex from 'rehype-katex'
import remarkGfm from 'remark-gfm'
import remarkMath from 'remark-math'
import { Check, Copy } from 'lucide-react'
import { useState } from 'react'

import { Button } from '@/components/ui/button'
import { useGroupWorkspaceRoot } from '@/hooks/useGroupFiles'
import { cn } from '@/lib/utils'
import {
  WORKSPACE_FILE_SCHEME,
  joinWorkspaceAbsPath,
  parseWorkspaceFileHref,
  remarkWorkspaceFiles,
} from '@/lib/workspaceFileLink'
import { useFileNavStore } from '@/stores/fileNavStore'

interface MarkdownMessageProps {
  content: string
  isUser?: boolean
  /** When set, file paths in the text become links that open the group's workspace panel. */
  groupId?: string
}

function extractText(node: unknown): string {
  if (typeof node === 'string' || typeof node === 'number') return String(node)
  if (!node || typeof node !== 'object') return ''
  if ('props' in node) {
    const props = (node as { props?: { children?: unknown } }).props
    return extractText(props?.children)
  }
  if (Array.isArray(node)) return node.map(extractText).join('')
  return ''
}

function CodeBlock({ className, children }: React.HTMLAttributes<HTMLElement>) {
  const [copied, setCopied] = useState(false)
  const language = /language-(\w+)/.exec(className ?? '')?.[1]
  const code = extractText(children).replace(/\n$/, '')

  const copyCode = async () => {
    await navigator.clipboard.writeText(code)
    setCopied(true)
    window.setTimeout(() => setCopied(false), 1200)
  }

  return (
    <div className="my-3 overflow-hidden rounded-xl border border-border bg-foreground text-background shadow-sm">
      <div className="flex items-center justify-between border-b border-background/10 bg-background/10 px-3 py-1.5 text-[11px] uppercase tracking-[0.16em] text-background/70">
        <span>{language ?? 'code'}</span>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-[11px] text-background/80 hover:bg-background/15 hover:text-background"
          onClick={() => void copyCode()}
          aria-label="Copy code block"
        >
          {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
          {copied ? 'Copied' : 'Copy'}
        </Button>
      </div>
      <pre className="overflow-x-auto p-3 text-xs leading-relaxed">
        <code className={className}>{children}</code>
      </pre>
    </div>
  )
}

export function MarkdownMessage({ content, isUser = false, groupId }: MarkdownMessageProps) {
  const openFile = useFileNavStore((s) => s.openFile)
  const rootQuery = useGroupWorkspaceRoot(groupId)
  const root = rootQuery.data
  const remarkPlugins = groupId
    ? [remarkGfm, remarkMath, remarkWorkspaceFiles]
    : [remarkGfm, remarkMath]
  return (
    <div
      className={cn(
        'min-w-0 break-words text-sm leading-6',
        isUser ? 'text-primary-foreground' : 'text-foreground',
      )}
    >
      <ReactMarkdown
        remarkPlugins={remarkPlugins}
        rehypePlugins={[rehypeKatex]}
        urlTransform={(url) =>
          url.startsWith(WORKSPACE_FILE_SCHEME) ? url : defaultUrlTransform(url)
        }
        components={{
          p: ({ children }) => <p className="mb-2 last:mb-0">{children}</p>,
          a: ({ children, href }) => {
            const rel = groupId ? parseWorkspaceFileHref(href) : null
            if (rel && groupId) {
              return (
                <button
                  type="button"
                  title={joinWorkspaceAbsPath(root?.root, root?.separator, rel)}
                  onClick={() => openFile(groupId, rel)}
                  className={cn(
                    'inline cursor-pointer break-all font-medium underline underline-offset-4',
                    isUser ? 'text-primary-foreground' : 'text-primary',
                  )}
                >
                  {children}
                </button>
              )
            }
            return (
              <a
                href={href}
                target="_blank"
                rel="noreferrer"
                className={cn(
                  'font-medium underline underline-offset-4',
                  isUser ? 'text-primary-foreground' : 'text-primary',
                )}
              >
                {children}
              </a>
            )
          },
          ul: ({ children }) => <ul className="my-2 list-disc space-y-1 pl-5">{children}</ul>,
          ol: ({ children }) => <ol className="my-2 list-decimal space-y-1 pl-5">{children}</ol>,
          li: ({ children }) => <li className="pl-1">{children}</li>,
          blockquote: ({ children }) => (
            <blockquote
              className={cn(
                'my-3 border-l-2 pl-3 italic',
                isUser ? 'border-primary-foreground/50' : 'border-primary/50 text-muted-foreground',
              )}
            >
              {children}
            </blockquote>
          ),
          hr: () => <hr className="my-4 border-border" />,
          h1: ({ children }) => <h1 className="mb-2 mt-3 text-lg font-semibold">{children}</h1>,
          h2: ({ children }) => <h2 className="mb-2 mt-3 text-base font-semibold">{children}</h2>,
          h3: ({ children }) => <h3 className="mb-1.5 mt-3 text-sm font-semibold">{children}</h3>,
          table: ({ children }) => (
            <div className="my-3 overflow-x-auto rounded-lg border border-border">
              <table className="w-full border-collapse text-left text-xs">{children}</table>
            </div>
          ),
          th: ({ children }) => <th className="border-b border-border bg-muted px-3 py-2 font-semibold">{children}</th>,
          td: ({ children }) => <td className="border-b border-border px-3 py-2 last:border-b-0">{children}</td>,
          pre: ({ children }) => <>{children}</>,
          code: ({ className, children }) => {
            if (className?.startsWith('language-')) {
              return <CodeBlock className={className}>{children}</CodeBlock>
            }
            return (
              <code
                className={cn(
                  'rounded px-1.5 py-0.5 text-[0.85em]',
                  isUser ? 'bg-primary-foreground/15' : 'bg-muted text-foreground',
                )}
              >
                {children}
              </code>
            )
          },
        }}
      >
        {content || ' '}
      </ReactMarkdown>
    </div>
  )
}
