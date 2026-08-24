import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it } from 'vitest'

import { AgentAvatar } from '@/components/chat/AgentAvatar'
import { TooltipProvider } from '@/components/ui/tooltip'
import i18n from '@/i18n'

afterEach(async () => {
  cleanup()
  await i18n.changeLanguage('en-US')
})

describe('AgentAvatar', () => {
  it('renders a selected preset mark instead of initials', () => {
    const { container } = render(
      <AgentAvatar name="Builder" avatarUrl="preset:loom" />,
    )

    expect(container.querySelector('svg')).not.toBeNull()
    expect(container).not.toHaveTextContent('B')
  })

  it('falls back to initials for a preset id it no longer ships', () => {
    render(<AgentAvatar name="Builder" avatarUrl="preset:robot" />)

    expect(screen.getByText('B')).toBeVisible()
  })

  it('renders presets for user avatars too', () => {
    const { container } = render(
      <AgentAvatar name="Alice" kind="user" avatarUrl="preset:prism" />,
    )

    expect(container.querySelector('svg')).not.toBeNull()
    expect(container).not.toHaveTextContent('A')
  })

  it('uses the bot icon instead of initials for the system Assistant', () => {
    const { container } = render(<AgentAvatar name="AG Assistant" kind="system" />)

    expect(screen.getByLabelText('AG Assistant')).toBeVisible()
    expect(screen.queryByText('AA')).toBeNull()
    expect(container.querySelector('svg')).not.toBeNull()
  })

  it('frames an unknown context source while preserving its raw value after a locale switch', async () => {
    const user = userEvent.setup()
    await i18n.changeLanguage('en-US')
    render(
      <TooltipProvider delayDuration={0}>
        <AgentAvatar
          name="Researcher"
          contextUsage={{
            input_tokens: 10,
            output_tokens: null,
            total_tokens: null,
            context_window_tokens: 100,
            output_reserve_tokens: null,
            ratio: 0.1,
            source: 'future_context_source',
          }}
        />
      </TooltipProvider>,
    )

    await user.hover(screen.getByText('R'))
    expect((await screen.findAllByText('Source: future_context_source'))[0]).toBeVisible()

    await i18n.changeLanguage('zh-CN')
    expect((await screen.findAllByText('来源：future_context_source'))[0]).toBeVisible()
  })
})
