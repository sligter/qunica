/**
 * Byte plumbing shared by every terminal transport.
 *
 * PTY output crosses the wire as Base64 because it is not guaranteed to be
 * valid UTF-8 — a multi-byte character can be split across two reads — and
 * input is chunked on code-point boundaries so a large paste never cuts a
 * character in half.
 */

const INPUT_CHUNK_SIZE = 16 * 1024
const BASE64_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'

function decodeBase64WithoutPlatformApi(value: string): Uint8Array {
  const normalized = value.replace(/\s/g, '')
  if (normalized.length % 4 === 1 || /[^A-Za-z0-9+/=]/.test(normalized)) {
    throw new Error('Invalid Base64 terminal output')
  }

  const padding = normalized.endsWith('==') ? 2 : normalized.endsWith('=') ? 1 : 0
  const outputLength = Math.floor((normalized.length * 3) / 4) - padding
  const output = new Uint8Array(outputLength)
  let outputIndex = 0

  for (let index = 0; index < normalized.length; index += 4) {
    const a = BASE64_ALPHABET.indexOf(normalized[index] ?? '')
    const b = BASE64_ALPHABET.indexOf(normalized[index + 1] ?? '')
    const c = normalized[index + 2] === '=' ? 0 : BASE64_ALPHABET.indexOf(normalized[index + 2] ?? '')
    const d = normalized[index + 3] === '=' ? 0 : BASE64_ALPHABET.indexOf(normalized[index + 3] ?? '')
    if (a < 0 || b < 0 || c < 0 || d < 0) {
      throw new Error('Invalid Base64 terminal output')
    }

    const bits = (a << 18) | (b << 12) | (c << 6) | d
    if (outputIndex < outputLength) output[outputIndex++] = (bits >> 16) & 0xff
    if (outputIndex < outputLength) output[outputIndex++] = (bits >> 8) & 0xff
    if (outputIndex < outputLength) output[outputIndex++] = bits & 0xff
  }

  return output
}

export function decodeBase64Bytes(value: string): Uint8Array {
  if (typeof globalThis.atob !== 'function') {
    return decodeBase64WithoutPlatformApi(value)
  }

  const binary = globalThis.atob(value)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return bytes
}

function utf8CodePointSize(value: string, index: number): { bytes: number; codeUnits: number } {
  const first = value.charCodeAt(index)
  if (first <= 0x7f) return { bytes: 1, codeUnits: 1 }
  if (first <= 0x7ff) return { bytes: 2, codeUnits: 1 }
  if (first >= 0xd800 && first <= 0xdbff) {
    const second = value.charCodeAt(index + 1)
    if (second >= 0xdc00 && second <= 0xdfff) {
      return { bytes: 4, codeUnits: 2 }
    }
  }
  return { bytes: 3, codeUnits: 1 }
}

/** Replace unpaired surrogates, which cannot be encoded as UTF-8. */
export function toWellFormedUnicode(value: string): string {
  let normalized = ''
  let segmentStart = 0
  let index = 0

  while (index < value.length) {
    const codeUnit = value.charCodeAt(index)
    if (codeUnit >= 0xd800 && codeUnit <= 0xdbff) {
      const next = value.charCodeAt(index + 1)
      if (next >= 0xdc00 && next <= 0xdfff) {
        index += 2
        continue
      }
    } else if (codeUnit < 0xdc00 || codeUnit > 0xdfff) {
      index += 1
      continue
    }

    normalized += `${value.slice(segmentStart, index)}�`
    index += 1
    segmentStart = index
  }

  return segmentStart === 0
    ? value
    : normalized + value.slice(segmentStart)
}

export function* utf8InputChunks(value: string): Generator<string> {
  if (value.length === 0) {
    yield value
    return
  }

  let chunkStart = 0
  let chunkBytes = 0
  let index = 0
  while (index < value.length) {
    const point = utf8CodePointSize(value, index)
    if (chunkBytes + point.bytes > INPUT_CHUNK_SIZE) {
      yield value.slice(chunkStart, index)
      chunkStart = index
      chunkBytes = 0
      continue
    }
    chunkBytes += point.bytes
    index += point.codeUnits
  }
  yield value.slice(chunkStart)
}
