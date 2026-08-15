import ReactMarkdown, { defaultUrlTransform } from 'react-markdown'
import rehypeKatex from 'rehype-katex'
import remarkGfm from 'remark-gfm'
import remarkMath from 'remark-math'
import { Check, Copy } from 'lucide-react'
import { memo, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { useGroupWorkspaceRoot } from '@/hooks/useGroupFiles'
import { cn } from '@/lib/utils'
import {
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
  const { t } = useTranslation('chat')
  const [copied, setCopied] = useState(false)
  const language = /language-(\w+)/.exec(className ?? '')?.[1]
  const code = extractText(children).replace(/\n$/, '')

  const copyCode = async () => {
    await navigator.clipboard.writeText(code)
    setCopied(true)
    window.setTimeout(() => setCopied(false), 1200)
  }

  return (
    <div className="my-3 min-w-0 max-w-full overflow-hidden rounded-xl border border-border bg-code text-code-foreground shadow-sm">
      <div className="flex items-center justify-between border-b border-code-foreground/10 bg-code-foreground/10 px-3 py-1.5 text-2xs uppercase tracking-[0.16em] text-code-foreground/70">
        <span>{language ?? t('messages.code')}</span>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-2xs text-code-foreground/80 hover:bg-code-foreground/15 hover:text-code-foreground"
          onClick={() => void copyCode()}
          aria-label={t('messages.copyCode')}
        >
          {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
          {copied ? t('messages.copied') : t('messages.copy')}
        </Button>
      </div>
      <pre className="max-w-full overflow-x-auto p-3 text-xs leading-relaxed">
        <code className={className}>{children}</code>
      </pre>
    </div>
  )
}

const REHYPE_PLUGINS = [rehypeKatex]
const BASE_REMARK_PLUGINS = [remarkGfm, remarkMath]
const WORKSPACE_REMARK_PLUGINS = [remarkGfm, remarkMath, remarkWorkspaceFiles]

/**
 * Memoized: a conversation re-renders on every streamed token, and without this
 * each one re-runs the whole markdown pipeline for every message already on
 * screen — the parse cost of the backlog, once per token.
 */
export const MarkdownMessage = memo(function MarkdownMessage({
  content,
  isUser = false,
  groupId,
}: MarkdownMessageProps) {
  const openFile = useFileNavStore((s) => s.openFile)
  const rootQuery = useGroupWorkspaceRoot(groupId)
  const root = rootQuery.data
  const remarkPlugins = groupId ? WORKSPACE_REMARK_PLUGINS : BASE_REMARK_PLUGINS
  const components = useMemo(
    () => ({
      p: ({ children }: { children?: React.ReactNode }) => <p className="mb-2 last:mb-0">{children}</p>,
      a: ({ children, href }: { children?: React.ReactNode; href?: string }) => {
        const rel = groupId ? parseWorkspaceFileHref(href, root?.root) : null
        if (rel && groupId) {
          return (
            <button
              type="button"
              title={joinWorkspaceAbsPath(root?.root, root?.separator, rel)}
              onClick={() => openFile(groupId, rel)}
              className={cn(
                'inline cursor-pointer break-all font-medium underline underline-offset-4',
                isUser ? 'text-current' : 'text-primary',
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
              'break-all font-medium underline underline-offset-4',
              isUser ? 'text-current' : 'text-primary',
            )}
          >
            {children}
          </a>
        )
      },
      ul: ({ children }: { children?: React.ReactNode }) => <ul className="my-2 list-disc space-y-1 pl-5">{children}</ul>,
      ol: ({ children }: { children?: React.ReactNode }) => <ol className="my-2 list-decimal space-y-1 pl-5">{children}</ol>,
      li: ({ children }: { children?: React.ReactNode }) => <li className="pl-1">{children}</li>,
      blockquote: ({ children }: { children?: React.ReactNode }) => (
        <blockquote
          className={cn(
            'my-3 border-l-2 pl-3 italic',
            isUser ? 'border-current/50' : 'border-primary/50 text-muted-foreground',
          )}
        >
          {children}
        </blockquote>
      ),
      hr: () => <hr className="my-4 border-border" />,
      h1: ({ children }: { children?: React.ReactNode }) => <h1 className="font-serif mb-2 mt-3 text-lg font-semibold">{children}</h1>,
      h2: ({ children }: { children?: React.ReactNode }) => <h2 className="mb-2 mt-3 text-base font-semibold">{children}</h2>,
      h3: ({ children }: { children?: React.ReactNode }) => <h3 className="mb-1.5 mt-3 text-sm font-semibold">{children}</h3>,
      table: ({ children }: { children?: React.ReactNode }) => (
        <div className="my-3 min-w-0 max-w-full overflow-x-auto rounded-lg border border-border">
          <table className="w-full border-collapse text-left text-xs">{children}</table>
        </div>
      ),
      th: ({ children }: { children?: React.ReactNode }) => <th className="border-b border-border bg-muted px-3 py-2 font-semibold">{children}</th>,
      td: ({ children }: { children?: React.ReactNode }) => <td className="border-b border-border px-3 py-2 last:border-b-0">{children}</td>,
      pre: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
      code: ({ className, children }: React.HTMLAttributes<HTMLElement>) => {
        if (className?.startsWith('language-')) {
          return <CodeBlock className={className}>{children}</CodeBlock>
        }
        return (
          <code
            className={cn(
              'break-all rounded px-1.5 py-0.5 text-[0.85em]',
              isUser ? 'bg-current/15 text-current' : 'bg-muted text-foreground',
            )}
          >
            {children}
          </code>
        )
      },
    }),
    [groupId, isUser, openFile, root?.root, root?.separator],
  )

  return (
    <div
      className={cn(
        'min-w-0 max-w-full break-words [overflow-wrap:anywhere] text-sm leading-6',
        isUser && 'whitespace-pre-wrap',
        isUser ? 'chat-user-message text-current' : 'text-foreground',
      )}
    >
      <ReactMarkdown
        remarkPlugins={remarkPlugins}
        rehypePlugins={REHYPE_PLUGINS}
        urlTransform={(url) =>
          groupId && parseWorkspaceFileHref(url, root?.root)
            ? url
            : defaultUrlTransform(url)
        }
        components={components}
      >
        {content || ' '}
      </ReactMarkdown>
    </div>
  )
})
