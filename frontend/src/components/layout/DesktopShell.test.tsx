import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { DesktopShell } from './DesktopShell'
import { useDesktopConnection } from '@/lib/desktopConnection'
import '@/i18n'
const mocks=vi.hoisted(()=>({invoke:vi.fn(),check:vi.fn()}))
vi.mock('@tauri-apps/api/core',()=>({invoke:mocks.invoke}))
vi.mock('@/lib/realtime/instance',()=>({checkInstance:mocks.check}))
beforeEach(()=>{mocks.invoke.mockReset().mockResolvedValue(undefined);mocks.check.mockReset().mockResolvedValue(undefined)})
afterEach(()=>{cleanup();useDesktopConnection.setState({connection:{mode:'local'}})})

it('opens a remote workspace without starting the local backend',async()=>{
  useDesktopConnection.setState({connection:{mode:'remote',origin:'https://remote.example'}})
  render(<DesktopShell><div>Workspace</div></DesktopShell>)
  expect(await screen.findByText('Workspace')).toBeInTheDocument()
  expect(mocks.check).toHaveBeenCalledWith('https://remote.example')
  expect(mocks.invoke).not.toHaveBeenCalled()
})
it('waits for local startup before mounting authenticated routes',async()=>{
  let resolve!:()=>void
  mocks.invoke.mockReturnValue(new Promise<void>(done=>{resolve=done}))
  render(<DesktopShell><div>Workspace</div></DesktopShell>)
  expect(screen.queryByText('Workspace')).toBeNull()
  resolve()
  expect(await screen.findByText('Workspace')).toBeInTheDocument()
  expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('backend_start')
})
it('keeps server selection available when local startup fails',async()=>{
  mocks.invoke.mockRejectedValue(new Error('Port unavailable'))
  render(<DesktopShell><div>Workspace</div></DesktopShell>)
  expect(await screen.findByRole('alert')).toHaveTextContent('Port unavailable')
  expect(screen.getByLabelText(/Server address|服务器地址/)).toBeInTheDocument()
  expect(screen.queryByText('Workspace')).toBeNull()
})
