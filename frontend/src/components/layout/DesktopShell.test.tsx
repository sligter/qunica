import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { DesktopShell } from './DesktopShell'
import { DesktopInstanceSwitcher } from './DesktopInstanceSwitcher'
import { useDesktopConnection } from '@/lib/desktopConnection'
import { useAuthStore } from '@/stores/authStore'
import '@/i18n'
const mocks=vi.hoisted(()=>({invoke:vi.fn(),check:vi.fn()}))
vi.mock('@tauri-apps/api/core',()=>({invoke:mocks.invoke}))
vi.mock('@/lib/realtime/instance',()=>({checkInstance:mocks.check}))
beforeEach(()=>{localStorage.setItem('qunica:desktop-connection:v1', JSON.stringify({mode:'local'}));mocks.invoke.mockReset().mockResolvedValue(undefined);mocks.check.mockReset().mockResolvedValue(undefined)})
afterEach(()=>{cleanup();localStorage.clear();useDesktopConnection.setState({connection:{mode:'local'}});vi.restoreAllMocks()})

it('offers local mode before starting services and preserves the existing login',async()=>{
  localStorage.removeItem('qunica:desktop-connection:v1')
  const logout = vi.spyOn(useAuthStore.getState(), 'logout')
  render(<DesktopShell><div>Workspace</div></DesktopShell>)
  expect(screen.getByText(/无需 Docker|No Docker required/)).toBeInTheDocument()
  expect(mocks.invoke).not.toHaveBeenCalled()
  expect(mocks.check).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button',{name:/进入本地模式|Enter local mode/}))
  expect(await screen.findByText('Workspace')).toBeInTheDocument()
  expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('backend_start')
  expect(logout).not.toHaveBeenCalled()
  expect(JSON.parse(localStorage.getItem('qunica:desktop-connection:v1')!)).toEqual({mode:'local'})
})

it('retries local startup without discarding credentials',async()=>{
  mocks.invoke.mockRejectedValueOnce(new Error('Database unavailable'))
  const logout = vi.spyOn(useAuthStore.getState(), 'logout')
  render(<DesktopShell><div>Workspace</div></DesktopShell>)
  expect(await screen.findByRole('alert')).toHaveTextContent('Database unavailable')
  fireEvent.click(screen.getByRole('button',{name:/进入本地模式|Enter local mode/}))
  expect(await screen.findByText('Workspace')).toBeInTheDocument()
  expect(mocks.invoke).toHaveBeenCalledTimes(2)
  expect(logout).not.toHaveBeenCalled()
  expect(screen.queryByRole('alert')).toBeNull()
})

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

it('exposes saved instances through the sidebar control while keeping the workspace free of a top bar', async () => {
  const logout = vi.spyOn(useAuthStore.getState(), 'logout')
  render(<DesktopShell><DesktopInstanceSwitcher /><div>Workspace</div></DesktopShell>)
  expect(await screen.findByText('Workspace')).toBeInTheDocument()
  expect(screen.queryByText(/No Docker required|无需 Docker/)).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: /Switch instance|切换实例/ }))
  fireEvent.click(screen.getByRole('button', { name: /^(Local|本地)$/ }))
  expect(logout).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: /Switch instance|切换实例/ }))
  fireEvent.click(screen.getByRole('button', { name: /Add server|添加服务器/ }))
  expect(screen.getByLabelText(/Server address|服务器地址/)).toBeVisible()
  expect(screen.getByRole('button', { name: /Back to workspace|返回工作台/ })).toBeVisible()
})
