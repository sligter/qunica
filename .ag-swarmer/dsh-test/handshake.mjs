import { spawn } from 'node:child_process'

const bin = process.env.BIN
const config = process.env.CONFIG
const cwd = process.env.TD

const child = spawn(bin, ['--config', config], {
  cwd,
  stdio: ['pipe', 'pipe', 'pipe'],
  shell: true,
})

let stderr = ''
child.stderr.on('data', (d) => {
  const s = d.toString()
  stderr += s
  process.stdout.write('STDERR: ' + s)
})
child.on('exit', (code, sig) => {
  console.log('EXIT code=' + code + ' sig=' + sig)
})

setTimeout(() => {
  const init = JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} })
  console.log('SENDING ' + init)
  child.stdin.write(init + '\n')
}, 2500)

setTimeout(() => {
  console.log('DONE. stderr-bytes=' + stderr.length)
  child.kill('SIGKILL')
  setTimeout(() => process.exit(0), 300)
}, 9000)
