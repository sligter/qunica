import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { MarkdownMessage } from '@/components/chat/MarkdownMessage'
import { useFileNavStore } from '@/stores/fileNavStore'

vi.mock('@/hooks/useGroupFiles', () => ({
  useGroupWorkspaceRoot: () => ({
    data: { root: '\\\\?\\D:\\file\\learn\\AIGC\\ag-swarmer', separator: '\\' },
  }),
}))

afterEach(() => {
  cleanup()
  useFileNavStore.setState({ request: null })
})

describe('MarkdownMessage overflow containment', () => {
  it('highlights valid group member mentions outside code and links', () => {
    const { container } = render(
      <MarkdownMessage
        mentionNames={['Plan', 'Planner']}
        content={'Ask @planner, not @PlannerX or `@Planner` or [@Planner](https://example.com).'}
      />,
    )

    expect(Array.from(container.querySelectorAll('span.chat-mention')).map((node) => node.textContent))
      .toEqual(['@planner'])
  })

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

  it('opens an absolute workspace file link instead of navigating the webview', () => {
    render(
      <MarkdownMessage
        groupId="group-1"
        content="[workspace_files.rs](D:/file/learn/AIGC/ag-swarmer/backend-rs/crates/backend/src/api/workspace_files.rs:497)"
      />,
    )

    const file = screen.getByRole('button', { name: 'workspace_files.rs' })
    fireEvent.click(file)

    expect(screen.queryByRole('link', { name: 'workspace_files.rs' })).not.toBeInTheDocument()
    expect(useFileNavStore.getState().request).toMatchObject({
      groupId: 'group-1',
      path: 'backend-rs/crates/backend/src/api/workspace_files.rs',
    })
  })
})
