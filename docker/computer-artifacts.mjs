// Only ordinary files inside this computer's workspace become downloadable.
import { lstatSync, readdirSync, realpathSync, openSync, fstatSync, readSync, closeSync, constants } from 'node:fs'
import { createHash } from 'node:crypto'
import { resolve, relative, isAbsolute } from 'node:path'

export const MAX_ARTIFACT_BYTES = 1000 * 1024 * 1024
export const ARTIFACT_CHUNK_BYTES = 49152
const digests = new Map()
const identity = stat => [stat.dev, stat.ino, stat.size, stat.mtimeNs, stat.ctimeNs].join(':')

// Note: hash at transfer start with a bounded buffer, not once per chunk.
// Refresh even with unchanged stats: filesystems may coalesce rapid timestamps.
// Identity checks and the receiver's final digest reject edits between chunks.
export function artifactChunk(root, { path: name, offset = 0 }) {
  if (!Number.isSafeInteger(offset) || offset < 0) throw new Error('Invalid artifact offset')
  const path = realpathSync(workspaceFilename(root, name))
  const file = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK)
  try {
    const actual = realpathSync('/proc/self/fd/' + file)
    if (!actual.startsWith(root + '/')) throw new Error('Artifact path denied')
    const opened = fstatSync(file, { bigint: true })
    if (!opened.isFile() || opened.size > BigInt(MAX_ARTIFACT_BYTES)) throw new Error('Artifact exceeds 1000 MiB or is not a regular file')
    const size = Number(opened.size)
    if (offset > size) throw new Error('Invalid artifact offset')
    const version = identity(opened)
    let cached = digests.get(path)
    if (offset === 0 || cached?.version !== version) {
      const hash = createHash('sha256')
      const buffer = Buffer.alloc(Math.min(size, 1024 * 1024))
      let position = 0
      while (position < size) {
        const count = readSync(file, buffer, 0, Math.min(buffer.length, size - position), position)
        if (!count) throw new Error('Artifact changed during read')
        hash.update(buffer.subarray(0, count))
        position += count
      }
      cached = { version, sha256: hash.digest('hex') }
    }
    const data = Buffer.alloc(Math.min(ARTIFACT_CHUNK_BYTES, size - offset))
    let count = 0
    while (count < data.length) {
      const read = readSync(file, data, count, data.length - count, offset + count)
      if (!read) throw new Error('Artifact changed during read')
      count += read
    }
    if (identity(fstatSync(file, { bigint: true })) !== version) throw new Error('Artifact changed during read')
    digests.delete(path)
    digests.set(path, cached)
    if (digests.size > 32) digests.delete(digests.keys().next().value)
    return { path: relative(root, path), size, sha256: cached.sha256, data: data.toString('base64') }
  } finally { closeSync(file) }
}

export function workspaceFilename(root, value) {
  if (typeof value !== 'string' || !value || value.includes('\0') || value.includes('\\') || isAbsolute(value) || /^[a-z]:/i.test(value) || value.split('/').includes('..')) throw new Error('Browser filename must stay inside the computer workspace')
  const target = resolve(root, value)
  let existing = target
  while (true) {
    try { existing = realpathSync(existing); break } catch (error) {
      if (error.code !== 'ENOENT') throw error
      const parent = resolve(existing, '..')
      if (parent === existing) throw error
      existing = parent
    }
  }
  if (existing !== root && !existing.startsWith(root + '/')) throw new Error('Browser filename escapes the computer workspace')
  return target
}

export function artifactFiles(root, extraPaths = []) {
  const paths = [...extraPaths, ...readdirSync(root + '/artifacts').map(name => 'artifacts/' + name)]
  const files = new Map()
  for (const name of paths) {
    if (files.size >= 200) break
    try {
      const path = workspaceFilename(root, name)
      const stat = lstatSync(path)
      if (!stat.isFile() || stat.isSymbolicLink() || realpathSync(path) !== path) continue
      const rel = relative(root, path)
      files.set(rel, { path: rel, size: stat.size })
    } catch { /* A missing/unsafe output never hides valid browser artifacts. */ }
  }
  return [...files.values()]
}
