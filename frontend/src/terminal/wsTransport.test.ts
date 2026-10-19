import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { useAuthStore } from '@/stores/authStore'
const mock=vi.hoisted(()=>({request:vi.fn(),create:vi.fn()}))
vi.mock('@/lib/realtime/client',()=>({realtimeClient:()=>({request:mock.request})}))
vi.mock('./httpTransport',async original=>({...await original<typeof import('./httpTransport')>(),terminalRequest:mock.create}))
import { createWsTerminalTransport } from './wsTransport'
import { ApiError } from '@/lib/api-v2/client'

class Socket {
  static OPEN=1
  static all:Socket[]=[]
  readyState=1
  bufferedAmount=0
  frames:Record<string,unknown>[]=[]
  onmessage?:(event:{data:string})=>void
  onclose?:()=>void
  onerror?:()=>void
  constructor(){Socket.all.push(this)}
  send(raw:string){this.frames.push(JSON.parse(raw))}
  close(){if(this.readyState===3)return;this.readyState=3;this.onclose?.()}
  receive(frame:Record<string,unknown>){this.onmessage?.({data:JSON.stringify(frame)})}
  authenticate(generation:string|null='1'){
    this.receive({type:'hello',protocol:'qunica.terminal.v1',server_id:'server'})
    this.receive({type:'authenticated',generation})
  }
}
beforeEach(()=>{
  vi.stubGlobal('WebSocket',Socket);Socket.all=[]
  useAuthStore.setState({token:'token'})
  mock.create.mockReset().mockResolvedValue({session_id:'session',shell_name:'shell',cwd:'/workspace'})
  mock.request.mockReset().mockResolvedValue({ticket:'ticket',server_id:'server'})
})
afterEach(()=>{useAuthStore.setState({token:null});vi.unstubAllGlobals();vi.useRealTimers()})
async function open(generation:string|null='1'){
  const transport=createWsTerminalTransport()
  const onEvent=vi.fn()
  const create=transport.create({conversationId:'group',cwd:'/workspace',cols:80,rows:24},onEvent)
  await vi.waitFor(()=>expect(Socket.all).toHaveLength(1))
  Socket.all[0].authenticate(generation)
  await create
  return {transport,onEvent,socket:Socket.all[0]}
}

it('resumes only rendered output and rejects uncertain and queued input on a new connection',async()=>{
  const {transport,socket,onEvent}=await open()
  socket.receive({type:'output',cursor:'1',event:{event:'output',data:{bytes_base64:'aGk='}}})
  const pending=transport.write('group','session','echo once\r')
  const failed=expect(pending).rejects.toMatchObject({code:'terminal.disconnected'})
  await vi.waitFor(()=>expect(socket.frames.some(frame=>frame.type==='input')).toBe(true))
  const queued=transport.write('group','session','do not replay\r')
  const queuedFailed=expect(queued).rejects.toMatchObject({code:'terminal.disconnected'})
  socket.close()
  window.dispatchEvent(new Event('online'))
  await vi.waitFor(()=>expect(Socket.all).toHaveLength(2))
  const restored=Socket.all[1]
  restored.authenticate('2')
  await failed;await queuedFailed
  expect(restored.frames[0]).toMatchObject({type:'authenticate',cursor:'1',session_id:'session',ticket:'ticket'})
  expect(restored.frames.some(frame=>frame.type==='input')).toBe(false)
  expect(mock.create).toHaveBeenCalledOnce()
  expect(onEvent.mock.calls.filter(([event])=>event.event==='output')).toHaveLength(1)
})

it('keeps an observer read-only while receiving output',async()=>{
  const {transport,socket,onEvent}=await open(null)
  await expect(transport.write('group','session','x')).rejects.toMatchObject({code:'terminal.lease_required'})
  socket.receive({type:'output',cursor:'1',event:{event:'output',data:{bytes_base64:'aGk='}}})
  expect(onEvent).toHaveBeenCalledWith({event:'control',data:{writable:false}})
  expect(onEvent.mock.calls.some(([event])=>event.event==='output')).toBe(true)
  expect(socket.frames.some(frame=>frame.type==='input')).toBe(false)
})

it.each([new TypeError('Failed to fetch'), new ApiError(503, 'unavailable', 'Try again')])('retries a transient ticket failure without recreating the PTY',async(error)=>{
  const {socket,onEvent}=await open()
  mock.request.mockRejectedValueOnce(error)
  socket.close()
  window.dispatchEvent(new Event('online'))
  await vi.waitFor(()=>expect(mock.request).toHaveBeenCalledTimes(2))
  // The next online notification may accelerate backoff, without replacing PTY.
  window.dispatchEvent(new Event('online'))
  await vi.waitFor(()=>expect(Socket.all).toHaveLength(2))
  Socket.all[1].authenticate('2')
  expect(mock.create).toHaveBeenCalledOnce()
  expect(onEvent.mock.calls.some(([event])=>event.event==='error')).toBe(false)
  expect(Socket.all[1].frames.some(frame=>frame.type==='input')).toBe(false)
})

it('stops retrying when the ticket endpoint rejects authentication',async()=>{
  const {socket,onEvent}=await open()
  mock.request.mockRejectedValueOnce(new ApiError(401,'session_revoked','Sign in again'))
  socket.close()
  window.dispatchEvent(new Event('online'))
  await vi.waitFor(()=>expect(onEvent).toHaveBeenLastCalledWith({event:'error',data:{code:'terminal.connection_failed',message:'Sign in again'}}))
  window.dispatchEvent(new Event('online'))
  expect(mock.request).toHaveBeenCalledTimes(2)
  expect(Socket.all).toHaveLength(1)
})

it('automatically backs off through a ticket failure without an online event',async()=>{
  const {socket,onEvent}=await open()
  vi.useFakeTimers()
  mock.request.mockRejectedValueOnce(new TypeError('offline'))
  socket.close()
  await vi.advanceTimersByTimeAsync(650)
  expect(mock.request).toHaveBeenCalledTimes(2)
  expect(Socket.all).toHaveLength(1)
  await vi.advanceTimersByTimeAsync(1_300)
  expect(mock.request).toHaveBeenCalledTimes(3)
  expect(Socket.all).toHaveLength(2)
  Socket.all[1].authenticate('2')
  expect(onEvent.mock.calls.some(([event])=>event.event==='error')).toBe(false)
})

it('explicitly releases control and requests it again without sending terminal input',async()=>{
  const {transport,socket,onEvent}=await open()
  const release=transport.setInputControl!('session',false)
  const request=socket.frames.at(-1)!
  expect(request.type).toBe('release_input')
  socket.receive({type:'lease',request_id:request.request_id,generation:null})
  await release
  expect(onEvent).toHaveBeenLastCalledWith({event:'control',data:{writable:false}})
  const acquire=transport.setInputControl!('session',true)
  const take=socket.frames.at(-1)!
  socket.receive({type:'lease',request_id:take.request_id,generation:'2'})
  await acquire
  expect(onEvent).toHaveBeenLastCalledWith({event:'control',data:{writable:true}})
  expect(socket.frames.some(frame=>frame.type==='input')).toBe(false)
})

it('reports a replay overflow and ignores callbacks from a signed-out device',async()=>{
  const {socket,onEvent}=await open()
  socket.receive({type:'output',cursor:'3',event:{event:'output',data:{bytes_base64:'aGk='}}})
  expect(onEvent).toHaveBeenLastCalledWith({event:'error',data:{code:'terminal.output_gap',message:expect.stringContaining('replay window')}})
  const count=onEvent.mock.calls.length
  useAuthStore.setState({token:null})
  socket.receive({type:'output',cursor:'1',event:{event:'output',data:{bytes_base64:'aGk='}}})
  expect(onEvent).toHaveBeenCalledTimes(count)
})
