import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { SettingsRow, SettingsSection } from '@/components/ui/settings-row'

describe('settings layout alignment', () => {
  afterEach(cleanup)

  it('fills the page width so its rules end where the page does', () => {
    const { container } = render(
      <SettingsSection title="Scheduler">
        <SettingsRow label="Turn timeout">
          <input aria-label="timeout" />
        </SettingsRow>
      </SettingsSection>,
    )

    // A cap here would stop the section's rules short of the header's own rule,
    // leaving a band of dead space down the right of every setting.
    const section = container.firstElementChild
    expect(section).toHaveClass('w-full')
    expect(section?.className).not.toMatch(/\bmax-w-/)
  })

  it('puts every inline control in one right-flushed column', () => {
    render(
      <SettingsRow label="Turn timeout">
        <input aria-label="timeout" />
      </SettingsRow>,
    )

    const control = screen.getByLabelText('timeout').parentElement
    // Fixed-width and right-flushed: a switch, a number box and a select then
    // all end on the same vertical line instead of three ragged ones.
    expect(control).toHaveClass('sm:w-72', 'sm:justify-end', 'sm:shrink-0')
  })

  it('stacks the row on a narrow window instead of squeezing the label', () => {
    const { container } = render(
      <SettingsRow label="Turn timeout">
        <input aria-label="timeout" />
      </SettingsRow>,
    )

    expect(container.firstElementChild).toHaveClass('flex-col', 'sm:flex-row')
    expect(screen.getByLabelText('timeout').parentElement).toHaveClass('w-full')
  })

  it('wraps a description at a reading measure though the row spans the page', () => {
    render(
      <SettingsRow label="Turn timeout" description="How long a turn may run.">
        <input aria-label="timeout" />
      </SettingsRow>,
    )

    expect(screen.getByText('How long a turn may run.')).toHaveClass('max-w-prose')
  })

  it('lets a stacked row fill the section so it ends on the same line', () => {
    const { container } = render(
      <SettingsRow label="Announcement" stacked>
        <textarea aria-label="announcement" />
      </SettingsRow>,
    )

    expect(container.firstElementChild).toHaveClass('space-y-1.5')
    expect(container.firstElementChild).not.toHaveClass('sm:w-72')
  })
})
