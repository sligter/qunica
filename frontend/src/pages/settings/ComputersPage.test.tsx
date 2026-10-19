import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'
import { ComputersPage } from './ComputersPage'

const mocks=vi.hoisted(()=>({request:vi.fn(),items:[] as unknown[],configured:true}))
vi.mock('@/lib/api-v2/client',()=>({fetchJson:(...args:unknown[])=>mocks.request(...args)}))
vi.mock('@/hooks/useGroups',()=>({useGroups:()=>({data:[{id:'group-1',name:'Team'}]})}))
vi.mock('@/hooks/useAgents',()=>({useAgents:()=>({data:[{id:'agent-1',name:'Built-in',runtime_kind:'llm'},{id:'agent-2',name:'External',runtime_kind:'acp'}]})}))
beforeEach(async()=>{
  await i18n.changeLanguage('en-US')
  useAuthStore.setState({token:'token'})
  mocks.items=[];mocks.configured=true
  mocks.request.mockReset().mockImplementation(async(path:string,opts?:{method?:string})=>{
    if(path==='/computers' && !opts?.method)return {configured:mocks.configured,items:mocks.items}
    if(path==='/computers/computer-1')return {generation:'4',status:'running',home_present:true,error_code:null}
    return {}
  })
})
afterEach(()=>{cleanup();useAuthStore.setState({token:null})})
function page(){const queries=new QueryClient({defaultOptions:{queries:{retry:false},mutations:{retry:false}}});render(<QueryClientProvider client={queries}><ComputersPage/></QueryClientProvider>);return userEvent.setup()}

it('retries the same creation intent after a lost response and requires explicit binding',async()=>{
  const original=mocks.request.getMockImplementation()!
  let attempts=0
  mocks.request.mockImplementation(async(path:string,opts?:{method?:string})=>{
    if(path==='/computers' && opts?.method==='POST' && attempts++===0)throw new Error('response lost')
    return original(path,opts)
  })
  const user=page()
  await user.type(screen.getByLabelText('Name'),'Build computer')
  await user.selectOptions(screen.getByLabelText('Bound to'),'group-1')
  await user.click(screen.getByRole('button',{name:'Create computer'}))
  expect(await screen.findByRole('alert')).toHaveTextContent('response lost')
  await user.click(screen.getByRole('button',{name:'Create computer'}))
  await waitFor(()=>expect(mocks.request.mock.calls.filter(([,opts])=>opts?.method==='POST')).toHaveLength(2))
  const calls=mocks.request.mock.calls.filter(([,opts])=>opts?.method==='POST')
  expect(calls[0][1].body).toEqual(calls[1][1].body)
  expect(calls[0][1].body).toMatchObject({name:'Build computer',binding:{kind:'team',id:'group-1'}})
  expect(mocks.request.mock.calls.some(([path])=>path==='/execution-environments/binding')).toBe(false)
})

it('binds explicitly and confirms a generation-fenced destroy before purging home',async()=>{
  mocks.items=[{id:'computer-1',name:'Build computer',binding:{kind:'team',id:'group-1'},selected:false}]
  const user=page()
  await user.click(await screen.findByRole('button',{name:'Use for execution'}))
  await waitFor(()=>expect(mocks.request).toHaveBeenCalledWith('/execution-environments/binding',expect.objectContaining({method:'PUT',body:{scope:'group',target_id:'group-1',execution_environment_id:'computer-1'}})))
  await user.click(screen.getByRole('button',{name:'Destroy'}))
  expect(screen.getByRole('alertdialog')).toHaveTextContent('home is retained')
  expect(mocks.request.mock.calls.some(([path])=>path.endsWith('/actions'))).toBe(false)
  await user.click(screen.getByLabelText('Permanently delete the computer’s home and files'))
  await user.click(screen.getByRole('button',{name:'Confirm'}))
  await waitFor(()=>expect(mocks.request).toHaveBeenCalledWith('/computers/computer-1/actions',expect.objectContaining({method:'POST',body:{action:'destroy',expected_generation:'4',confirm:'computer-1',purge_home:true}})))
})

it('explains a missing Supervisor and excludes unsupported ACP private targets',async()=>{
  mocks.configured=false
  const user=page()
  expect(await screen.findByText(/no Computer Supervisor/)).toBeVisible()
  expect(screen.getByRole('button',{name:'Create computer'})).toBeDisabled()
  await user.selectOptions(screen.getByLabelText('Access'),'private')
  expect(screen.getByRole('option',{name:'Built-in'})).toBeInTheDocument()
  expect(screen.queryByRole('option',{name:'External'})).not.toBeInTheDocument()
})
