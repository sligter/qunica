import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import { useDesktopConnection } from '@/lib/desktopConnection'
import { DesktopInstanceContext } from '@/lib/desktopInstanceContext'
import { DesktopInstanceSwitcher } from './DesktopInstanceSwitcher'

afterEach(() => {
  cleanup()
  useDesktopConnection.setState({ connection: { mode: 'local' }, instances: [{ mode: 'local' }] })
})

it('selects a saved destination and offers adding a server', async () => {
  await i18n.changeLanguage('en-US')
  useDesktopConnection.setState({ instances: [{ mode: 'local' }, { mode: 'remote', origin: 'https://build.example' }] })
  const select = vi.fn().mockResolvedValue(undefined)
  const add = vi.fn()
  const user = userEvent.setup()
  render(<DesktopInstanceContext.Provider value={{ select, add }}><DesktopInstanceSwitcher /></DesktopInstanceContext.Provider>)
  await user.click(screen.getByRole('button', { name: 'Switch instance' }))
  expect(screen.getByRole('button', { name: 'Local' })).toHaveAttribute('aria-pressed', 'true')
  await user.click(screen.getByRole('button', { name: 'https://build.example' }))
  expect(select).toHaveBeenCalledWith({ mode: 'remote', origin: 'https://build.example' })
  await user.click(screen.getByRole('button', { name: 'Switch instance' }))
  await user.click(screen.getByRole('button', { name: 'Add server' }))
  expect(add).toHaveBeenCalledOnce()
})
