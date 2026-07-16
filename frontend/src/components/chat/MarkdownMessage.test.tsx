import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { MarkdownMessage } from '@/components/chat/MarkdownMessage'

vi.mock('@/hooks/useGroupFiles', () => ({
  useGroupWorkspaceRoot: () => ({ data: undefined }),
}))

afterEach(cleanup)

describe('MarkdownMessage overflow containment', () => {
  it('wraps unbroken text and confines wide markdown elements', () => {
    const longToken = 'unbroken-content-'.repeat(24)
    const { container } = render(
      <MarkdownMessage
        content={`[${longToken}](https://example.com/${longToken})\n\n\`${longToken}\`\n\n\`\`\`text\n${longToken}\n\`\`\`\n\n| Column |\n| --- |\n| ${longToken} |`}
      />,
    )

    expect(container.firstElementChild).toHaveClass(
      'min-w-0',
      'max-w-full',
      '[overflow-wrap:anywhere]',
    )
    expect(screen.getByRole('link')).toHaveClass('break-all')

    const inlineCode = container.querySelector('p code')
    const codeBlock = container.querySelector('pre')
    const tableWrapper = container.querySelector('table')?.parentElement
    expect(inlineCode).toHaveClass('break-all')
    expect(codeBlock).toHaveClass('max-w-full', 'overflow-x-auto')
    expect(codeBlock?.parentElement).toHaveClass('min-w-0', 'max-w-full', 'overflow-hidden')
    expect(tableWrapper).toHaveClass('min-w-0', 'max-w-full', 'overflow-x-auto')
  })
})
