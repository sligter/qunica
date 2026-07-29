import { describe, expect, it } from 'vitest'

import { normalizeWindowsPath } from './folderPicker'

describe('normalizeWindowsPath', () => {
  it('removes Windows device path prefixes', () => {
    expect(normalizeWindowsPath('\\\\?\\D:\\video\\orca')).toBe('D:\\video\\orca')
    expect(normalizeWindowsPath('\\\\?\\UNC\\server\\share\\orca')).toBe(
      '\\\\server\\share\\orca',
    )
    expect(normalizeWindowsPath('D:\\video\\orca')).toBe('D:\\video\\orca')
  })
})
