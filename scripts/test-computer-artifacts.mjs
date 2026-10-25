import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync, symlinkSync, rmSync, truncateSync, openSync, closeSync, writeSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { artifactFiles, artifactChunk, workspaceFilename, MAX_ARTIFACT_BYTES, ARTIFACT_CHUNK_BYTES } from '../docker/computer-artifacts.mjs'

test('explicit screenshot filenames outside artifacts are indexed without scanning unrelated files', () => {
  const temp = mkdtempSync(join(tmpdir(), 'qunica-artifacts-'))
  try {
    const root = join(temp, 'workspace')
    mkdirSync(join(root, 'artifacts'), { recursive: true })
    writeFileSync(join(root, 'google.png'), 'image')
    writeFileSync(join(root, 'private.txt'), 'not a browser artifact')
    writeFileSync(join(root, 'artifacts', 'page.yml'), 'snapshot')
    const files = artifactFiles(root, ['google.png', 'missing.png', 'google.png'])
    assert.deepEqual(files.map(f => f.path).sort(), ['artifacts/page.yml', 'google.png'])
    assert.equal(files.find(f => f.path === 'google.png').size, 5)
    assert.equal(workspaceFilename(root, 'google.png'), join(root, 'google.png'))
  } finally { rmSync(temp, { recursive: true, force: true }) }
})

test('large artifact chunks use bounded memory, cache invalidation, and an inclusive 1000 MiB limit', () => {
  const temp = mkdtempSync(join(tmpdir(), 'qunica-artifact-transfer-'))
  try {
    const root = join(temp, 'workspace')
    mkdirSync(root)
    const path = join(root, 'large.bin')
    writeFileSync(path, '')
    truncateSync(path, MAX_ARTIFACT_BYTES)
    const before = process.memoryUsage().arrayBuffers
    const first = artifactChunk(root, { path: 'large.bin' })
    assert.equal(first.size, 1_048_576_000)
    assert.equal(Buffer.from(first.data, 'base64').length, ARTIFACT_CHUNK_BYTES)
    assert.ok(process.memoryUsage().arrayBuffers - before < 16 * 1024 * 1024, 'never allocate the entire file')
    const last = artifactChunk(root, { path: 'large.bin', offset: MAX_ARTIFACT_BYTES - 7 })
    assert.equal(last.sha256, first.sha256)
    assert.equal(Buffer.from(last.data, 'base64').length, 7)
    truncateSync(path, MAX_ARTIFACT_BYTES + 1)
    assert.throws(() => artifactChunk(root, { path: 'large.bin' }), /1000 MiB/)
    writeFileSync(path, Buffer.alloc(100_123, 1))
    const old = artifactChunk(root, { path: 'large.bin' })
    const fd = openSync(path, 'r+')
    try { writeSync(fd, Buffer.from([2]), 0, 1, 0) } finally { closeSync(fd) }
    const changed = artifactChunk(root, { path: 'large.bin' })
    assert.notEqual(changed.sha256, old.sha256, 'same-sized edits invalidate cached digests')
    const hash = createHash('sha256')
    for (let offset = 0; offset < changed.size; offset += ARTIFACT_CHUNK_BYTES) {
      hash.update(Buffer.from(artifactChunk(root, { path: 'large.bin', offset }).data, 'base64'))
    }
    assert.equal(hash.digest('hex'), changed.sha256)
    writeFileSync(join(root, 'empty'), '')
    assert.equal(artifactChunk(root, { path: 'empty' }).sha256, createHash('sha256').digest('hex'))
    for (const offset of [-1, 0.5, changed.size + 1]) assert.throws(() => artifactChunk(root, { path: 'large.bin', offset }))
    execFileSync('mkfifo', [join(root, 'pipe')])
    assert.throws(() => artifactChunk(root, { path: 'pipe' }), /regular file/)
    assert.throws(() => artifactChunk(root, { path: '../outside' }))
    symlinkSync('/etc/passwd', join(root, 'escape'))
    assert.throws(() => artifactChunk(root, { path: 'escape' }))
  } finally { rmSync(temp, { recursive: true, force: true }) }
})

test('filename containment rejects traversal, absolute paths and symlink escapes; unsafe entries do not hide valid artifacts', () => {
  const temp = mkdtempSync(join(tmpdir(), 'qunica-artifacts-'))
  try {
    const root = join(temp, 'workspace')
    mkdirSync(join(root, 'artifacts'), { recursive: true })
    writeFileSync(join(temp, 'secret'), 'secret')
    writeFileSync(join(root, 'artifacts', 'valid.txt'), 'valid')
    symlinkSync(join(temp, 'secret'), join(root, 'artifacts', 'escape'))
    symlinkSync(temp, join(root, 'outside'))
    for (const value of ['../secret', '/etc/passwd', 'C:/secret', 'x\\secret', 'outside/new.png', 'artifacts/escape']) {
      assert.throws(() => workspaceFilename(root, value), value)
    }
    assert.deepEqual(artifactFiles(root, ['../secret']).map(f => f.path), ['artifacts/valid.txt'])
  } finally { rmSync(temp, { recursive: true, force: true }) }
})
