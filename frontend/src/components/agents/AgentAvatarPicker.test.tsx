import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AgentAvatarPicker } from '@/components/agents/AgentAvatarPicker'
import { AgentAvatarArt } from '@/components/chat/AgentAvatarArt'
import { AGENT_AVATAR_PRESETS } from '@/lib/agentAvatar'
import i18n from '@/i18n'

afterEach(async () => {
  cleanup()
  await i18n.changeLanguage('en-US')
})

describe('preset avatar artwork', () => {
  it('draws a distinct mark for every preset', () => {
    const marks = AGENT_AVATAR_PRESETS.map((preset) => {
      const { container, unmount } = render(<AgentAvatarArt preset={preset} />)
      const svg = container.querySelector('svg')
      expect(svg, preset.id).not.toBeNull()
      // Background wash plus at least two drawn shapes, or the mark is a blank disc.
      expect(svg!.querySelectorAll('path, circle, ellipse, rect').length, preset.id)
        .toBeGreaterThan(2)
      const markup = svg!.innerHTML
      unmount()
      return markup
    })

    expect(new Set(marks).size).toBe(AGENT_AVATAR_PRESETS.length)
  })
})

describe('AgentAvatarPicker', () => {
  it('offers initials, every preset, and upload on one rail', () => {
    render(<AgentAvatarPicker value={null} name="Nova Ray" onChange={vi.fn()} />)

    const rail = screen.getByRole('group', { name: 'Avatar' })
    expect(rail.querySelectorAll('button')).toHaveLength(AGENT_AVATAR_PRESETS.length + 2)
    expect(screen.getByRole('button', { name: 'Initials' })).toHaveTextContent('NR')
    expect(screen.getByRole('button', { name: 'Upload image' })).toBeVisible()
  })

  it('reports the chosen preset and names it in the header', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    const { rerender } = render(
      <AgentAvatarPicker value={null} name="Nova" onChange={onChange} />,
    )
    expect(screen.getByText('Initials')).toBeVisible()

    await user.click(screen.getByRole('button', { name: 'Prism' }))
    expect(onChange).toHaveBeenCalledWith('preset:prism')

    rerender(<AgentAvatarPicker value="preset:prism" name="Nova" onChange={onChange} />)
    expect(screen.getByText('Prism')).toBeVisible()
  })

  it('shows an uploaded image on the upload tile instead of the add affordance', () => {
    const custom = 'data:image/webp;base64,AAAA'
    render(<AgentAvatarPicker value={custom} name="Nova" onChange={vi.fn()} />)

    const tile = screen.getByRole('button', { name: 'Custom image' })
    expect(tile.querySelector('img')).toHaveAttribute('src', custom)
    expect(screen.queryByRole('button', { name: 'Upload image' })).toBeNull()
  })
})
