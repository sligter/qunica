import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { ThinkingLevelControl } from '@/components/agents/ThinkingLevelControl'
import '@/i18n'

describe('ThinkingLevelControl', () => {
  afterEach(cleanup)

  it('maps the ascending slider positions to thinking levels', () => {
    const onChange = vi.fn()
    render(<ThinkingLevelControl value="high" onChange={onChange} />)

    const slider = screen.getByRole('slider', { name: 'Thinking level' })
    expect((slider as HTMLInputElement).value).toBe('3')
    expect(slider).toHaveAttribute('aria-valuetext', 'High')

    fireEvent.change(slider, { target: { value: '5' } })
    expect(onChange).toHaveBeenCalledWith('max')
  })
})
