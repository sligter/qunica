// Container-local broker. Only the Supervisor reaches it through Docker exec.
import http from 'node:http'
import net from 'node:net'
import { spawn, execFileSync } from 'node:child_process'
import { randomUUID, createHash } from 'node:crypto'
import { mkdirSync, readFileSync, statSync, realpathSync } from 'node:fs'
import { resolve } from 'node:path'
import { artifactFiles, artifactChunk, workspaceFilename } from './computer-artifacts.mjs'
const root = '/home/agent/workspace'
let pageVersion = randomUUID(), screenVersion = randomUUID(), browser, acp
const pending = new Map()
let rpcId = 0
const displays = new Map()
let stopping = false
let observedPage, lastAction, unchangedActions = 0
function pageHash(result) {
  const text = result?.content?.filter(part => part.type === 'text').map(part => part.text).join('\n') || ''
  const file = text.match(/\[Snapshot\]\((artifacts\/page-[A-Za-z0-9_.:-]+\.yml)\)/)?.[1]
  let snapshot = text.match(/### Snapshot\n([\s\S]*?)(?:\n### |$)/)?.[1]
  try { if (file) snapshot = readFileSync(resolve(root, file), 'utf8') } catch {}
  return snapshot ? createHash('sha256').update(snapshot.replace(/\[ref=[^\]]+\]/g, '')).digest('hex') : undefined
}
function uncertain(message) { return Object.assign(new Error(message), { uncertain: true }) }
process.on('SIGTERM', () => {
  if (stopping) return
  stopping = true
  const deadline = setTimeout(() => process.exit(1), 8000)
  void (async () => {
    try { if (browser) await rpc('tools/call', { name: 'browser_close', arguments: {} }) } catch {}
    kill(acp?.child)
    clearTimeout(deadline)
    process.exit(0)
  })()
})
function child(command, args, env = {}, cwd = root) {
  return spawn(command, args, { cwd, env: { ...process.env, ...env }, stdio: ['pipe', 'pipe', 'ignore'], detached: true })
}
function kill(process, signal = 'SIGKILL') { if (process) { try { globalThis.process.kill(-process.pid, signal) } catch {} } }
function rpc(method, params = {}) {
  if (!browser) {
    browser = child('playwright-mcp', ['--browser', 'chromium', '--executable-path', '/usr/bin/chromium', '--no-sandbox', '--host', '127.0.0.1', '--user-data-dir', '/home/agent/.qunica/browser', '--output-dir', root + '/artifacts', '--proxy-server', 'http://egress:3128', '--proxy-bypass', '127.0.0.1,localhost'])
    let buffer = ''
    browser.stdout.on('data', bytes => {
      buffer += bytes.toString()
      if (buffer.length > 4 * 1024 * 1024) { kill(browser); return }
      let newline
      while ((newline = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, newline); buffer = buffer.slice(newline + 1)
        try { const message = JSON.parse(line); if (message.id) { const pair = pending.get(message.id); pending.delete(message.id); message.error ? pair?.reject(new Error('Browser protocol error')) : pair?.resolve(message.result) } } catch {}
      }
    })
    browser.once('exit', () => { browser = undefined; for (const p of pending.values()) p.reject(uncertain('Browser stopped')); pending.clear() })
    return rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'qunica', version: '1' } }).then(() => {
      browser.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n')
      return rpc(method, params)
    })
  }
  const id = ++rpcId
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(uncertain('Browser timeout')) }, 45000)
    pending.set(id, { resolve: x => { clearTimeout(timer); resolve(x) }, reject: e => { clearTimeout(timer); reject(e) } })
    browser.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n')
  })
}
const allowed = new Set(['browser_navigate', 'browser_snapshot', 'browser_click', 'browser_type', 'browser_press_key', 'browser_select_option', 'browser_tabs', 'browser_file_upload', 'browser_wait_for', 'browser_take_screenshot', 'browser_handle_dialog'])
const readOnly = new Set(['browser_snapshot', 'browser_take_screenshot'])
// MCP resolves explicit screenshot filenames against cwd, not --output-dir.
// Track those outputs as well, without scanning or exporting the entire home.
const browserOutputs = new Set()
function artifacts() { return artifactFiles(root, [...browserOutputs]) }
async function call(body) {
  switch (body.op) {
    case 'browser': {
      if (body.tool === 'tools') return { tools: (await rpc('tools/list')).tools.filter(tool => allowed.has(tool.name)) }
      if (!allowed.has(body.tool)) throw new Error('Unsupported browser capability')
      if (!readOnly.has(body.tool) && body.tool !== 'browser_navigate' && body.expected_page !== pageVersion) throw new Error('stale_page: take a fresh snapshot')
      // Consume before dispatch: failed or repeated actions never reuse a ref.
      if (!readOnly.has(body.tool)) { pageVersion = randomUUID(); screenVersion = randomUUID() }
      const args = { ...(body.args || {}) }
      if (args.filename !== undefined) workspaceFilename(root, args.filename)
      if (args.ref && !args.target) args.target = args.ref
      delete args.ref; delete args.element
      const signature = JSON.stringify([body.tool, args])
      if (!readOnly.has(body.tool) && signature === lastAction && unchangedActions >= 3) throw Object.assign(new Error('Repeated browser action has no effect; inspect the page or request human control'), { code: 'action_repeat_limit' })
      const result = await rpc('tools/call', { name: body.tool, arguments: args })
      if (!result.isError && typeof args.filename === 'string') {
        browserOutputs.add(args.filename)
        if (browserOutputs.size > 200) browserOutputs.delete(browserOutputs.values().next().value)
      }
      const after = pageHash(result)
      if (!readOnly.has(body.tool)) {
        const checksDom = ['browser_click', 'browser_type', 'browser_select_option', 'browser_navigate', 'browser_tabs'].includes(body.tool)
        const noEffect = result.isError || (checksDom && after && after === observedPage)
        unchangedActions = signature === lastAction && noEffect ? unchangedActions + 1 : 0
        lastAction = signature
      } else if (after && after !== observedPage) unchangedActions = 0
      if (after) observedPage = after
      if (body.tool === 'browser_snapshot') pageVersion = randomUUID()
      return { page: pageVersion, result, artifacts: artifacts() }
    }
    case 'screen': {
      const image = '/tmp/qunica-screen.jpg'
      for (const quality of ['45', '25', '10', '1']) {
        execFileSync('scrot', ['-o', '-q', quality, image], { timeout: 5000 })
        if (statSync(image).size < 90000) break
      }
      if (statSync(image).size >= 90000) throw new Error('Screenshot exceeds output limit')
      screenVersion = randomUUID()
      return { screen: screenVersion, mime_type: 'image/jpeg', data: readFileSync(image).toString('base64'), width: 1280, height: 800 }
    }
    case 'input': {
      if (body.expected_screen !== screenVersion) throw new Error('stale_screen')
      screenVersion = randomUUID(); pageVersion = randomUUID()
      if (body.action === 'click' && Number.isInteger(body.x) && Number.isInteger(body.y) && body.x >= 0 && body.x < 1280 && body.y >= 0 && body.y < 800) execFileSync('xdotool', ['mousemove', String(body.x), String(body.y), 'click', '1'])
      else if (body.action === 'type' && typeof body.text === 'string' && body.text.length <= 4000) execFileSync('xdotool', ['type', '--clearmodifiers', '--', body.text])
      else if (body.action === 'key' && /^[A-Za-z0-9_+]{1,60}$/.test(body.key)) execFileSync('xdotool', ['key', '--clearmodifiers', body.key])
      else throw new Error('Unsupported desktop input')
      return { screen: screenVersion }
    }
    case 'quiesce':
      if (acp) {
        if (!acp.closed) {
          kill(acp.child, 'SIGTERM')
          await Promise.race([new Promise(resolve => acp.child.once('exit', resolve)), new Promise((_, reject) => setTimeout(() => reject(uncertain('ACP stop unconfirmed')), 10000))])
        }
        if (acp.exitCode === null || acp.exitCode >= 128) throw uncertain('ACP descendant stop unconfirmed')
        acp = undefined
      }
      pageVersion = randomUUID(); screenVersion = randomUUID()
      observedPage = undefined; lastAction = undefined; unchangedActions = 0
      return { stopped: true }
    case 'rfb_open': {
      if (displays.size >= 4) throw new Error('Desktop observer limit')
      const socket = net.connect(body.read_only ? 5901 : 5900, '127.0.0.1')
      const display = { socket, readOnly: !!body.read_only, chunks: [], size: 0, closed: false }
      const expiry = setTimeout(() => { socket.destroy(); displays.delete(id) }, 30_000)
      display.expiry = expiry
      socket.on('data', bytes => { display.chunks.push(bytes); display.size += bytes.length; if (display.size > 2 * 1024 * 1024) socket.destroy() })
      socket.on('error', () => { display.closed = true })
      socket.on('close', () => { display.closed = true })
      const id = randomUUID(); displays.set(id, display)
      return { id }
    }
    case 'rfb_exchange': {
      const display = displays.get(body.id)
      if (!display || display.readOnly !== !!body.read_only) throw new Error('Desktop channel expired')
      display.expiry.refresh()
      if (body.data) display.socket.write(Buffer.from(body.data, 'base64'))
      const buffer = Buffer.concat(display.chunks)
      const data = buffer.subarray(0, 49152)
      display.chunks = buffer.length > data.length ? [buffer.subarray(data.length)] : []
      display.size -= data.length
      return { data: data.toString('base64'), closed: display.closed }
    }
    case 'rfb_close': {
      const display = displays.get(body.id)
      if (display && display.readOnly === !!body.read_only) { clearTimeout(display.expiry); display.socket.destroy(); displays.delete(body.id) }
      return {}
    }
    case 'acp_start': {
      if (acp) throw new Error('ACP driver already active')
      if (!/^[a-zA-Z0-9-]{1,64}$/.test(body.agent)) throw new Error('Invalid agent')
      const cwd = realpathSync(resolve(root, body.cwd || '.'))
      if (cwd !== root && !cwd.startsWith(root + '/')) throw new Error('ACP workspace denied')
      if (!statSync(cwd).isDirectory()) throw new Error('ACP workspace unavailable')
      const home = '/home/agent/.qunica/acp/' + body.agent
      mkdirSync(home, { recursive: true, mode: 0o700 })
      const config = { $schema: 'https://opencode.ai/config.json', autoupdate: false, share: 'disabled', model: 'qunica/' + body.model, enabled_providers: ['qunica'], provider: { qunica: { npm: '@ai-sdk/openai-compatible', name: 'Qunica', options: { baseURL: 'http://egress:3128/qunica/model/v1', apiKey: body.model_grant, maxRetries: 0 }, models: { [body.model]: { name: body.model, limit: { context: 131072, output: 4096 } } } } }, mcp: body.notes_grant ? { 'qunica-group-notes': { type: 'remote', url: 'http://egress:3128/qunica/notes', headers: { Authorization: 'Bearer ' + body.notes_grant }, oauth: false } } : {}, permission: body.allow_tools ? 'allow' : 'deny' }
      const proc = child('python3', ['/opt/qunica/exec_guard.py', 'opencode', 'acp'], { HOME: home, XDG_CONFIG_HOME: home + '/config', XDG_DATA_HOME: home + '/data', XDG_CACHE_HOME: home + '/cache', OPENCODE_CONFIG_CONTENT: JSON.stringify(config), OPENCODE_DISABLE_AUTOUPDATE: 'true', OPENCODE_DISABLE_DEFAULT_PLUGINS: 'true', OPENCODE_DISABLE_MODELS_FETCH: 'true' }, cwd)
      acp = { child: proc, id: randomUUID(), lines: [], buffer: '', bytes: 0, closed: false }
      const session = acp
      proc.stdout.on('data', bytes => {
        session.buffer += bytes.toString(); session.bytes += bytes.length
        if (session.bytes > 2 * 1024 * 1024) { kill(proc); return }
        let newline
        while ((newline = session.buffer.indexOf('\n')) >= 0) { session.lines.push(session.buffer.slice(0, newline)); session.buffer = session.buffer.slice(newline + 1) }
      })
      proc.once('exit', (code) => { session.closed = true; session.exitCode = code })
      return { id: session.id }
    }
    case 'acp_exchange': {
      if (!acp || acp.id !== body.id) throw new Error('ACP session expired')
      if (body.lines?.length) for (const line of body.lines) { if (line.length > 512 * 1024) throw new Error('ACP message too large'); acp.child.stdin.write(line + '\n') }
      const lines = acp.lines.splice(0, 128); acp.bytes -= lines.reduce((n, line) => n + Buffer.byteLength(line) + 1, 0)
      return { lines, closed: acp.closed }
    }
    case 'artifact': return artifactChunk(root, body)
    default: throw new Error('Unsupported computer capability')
  }
}
// Serialization also protects browser references from concurrent observations.
let chain = Promise.resolve()
http.createServer((req, res) => {
  // A webpage in Chromium must not POST to the native control broker. Native
  // Supervisor calls have no Origin and always use JSON, including RFB polls.
  if (req.method !== 'POST' || req.url !== '/' || req.headers.origin !== undefined || !req.headers['content-type']?.startsWith('application/json')) {
    res.writeHead(403); res.end(); return
  }
  let chunks = [], bytes = 0
  req.on('data', c => { bytes += c.length; if (bytes > 1024 * 1024) req.destroy(); else chunks.push(c) })
  req.on('end', () => {
    let body
    try { body = JSON.parse(Buffer.concat(chunks)) } catch { res.writeHead(400); res.end(); return }
    const run = async () => {
      try { const value = await call(body); res.writeHead(200, { 'content-type': 'application/json' }); res.end(JSON.stringify(value)) }
      catch (error) { res.writeHead(409, { 'content-type': 'application/json' }); res.end(JSON.stringify({ error: error.message, code: error.code, uncertain: !!error.uncertain })) }
    }
    if (String(body.op).startsWith('rfb_')) void run()
    else chain = chain.catch(() => {}).then(run)
  })
}).listen(8765, '127.0.0.1')
