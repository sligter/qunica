import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { useSuppressNativeContextMenu } from '@/hooks/useSuppressNativeContextMenu'

function Harness() {
  useSuppressNativeContextMenu()
  return (
    <div>
      <p>Signed-out copy</p>
      <input aria-label="Email" type="email" />
      <input aria-label="Toggle" type="checkbox" />
      <textarea aria-label="Notes" />
      <div aria-label="Rich text" contentEditable suppressContentEditableWarning />
    </div>
  )
}

/** contextmenu carries no cancelable default in fireEvent's shorthand. */
function rightClick(target: Element): MouseEvent {
  const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
  fireEvent(target, event)
  return event
}

describe('useSuppressNativeContextMenu', () => {
  afterEach(cleanup)

  it('suppresses the webview menu on ordinary content', () => {
    render(<Harness />)

    expect(rightClick(screen.getByText('Signed-out copy')).defaultPrevented).toBe(true)
    expect(rightClick(screen.getByLabelText('Toggle')).defaultPrevented).toBe(true)
  })

  it('leaves editable surfaces their native menu', () => {
    render(<Harness />)

    expect(rightClick(screen.getByLabelText('Email')).defaultPrevented).toBe(false)
    expect(rightClick(screen.getByLabelText('Notes')).defaultPrevented).toBe(false)
    expect(rightClick(screen.getByLabelText('Rich text')).defaultPrevented).toBe(false)
  })

  it('stops suppressing once unmounted', () => {
    const { unmount } = render(<Harness />)
    const paragraph = screen.getByText('Signed-out copy')
    expect(rightClick(paragraph).defaultPrevented).toBe(true)

    unmount()
    document.body.appendChild(paragraph)
    expect(rightClick(paragraph).defaultPrevented).toBe(false)
  })
})
