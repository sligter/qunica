import { afterEach, describe, expect, it, vi } from 'vitest'

import { normalizeWindowsPath, pickFolder } from './folderPicker'

describe('pickFolder', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__')
  })

  // The OS dialog shows the folders of the machine running the browser. On a
  // Docker or VPS deployment those do not exist on the server, so offering it
  // at all is the bug.
  it('never opens the host dialog outside the desktop shell', async () => {
    const showDirectoryPicker = vi.fn()
    vi.stubGlobal('showDirectoryPicker', showDirectoryPicker)

    await expect(pickFolder()).resolves.toEqual({ kind: 'serverBrowse' })
    expect(showDirectoryPicker).not.toHaveBeenCalled()
  })

  it('uses the native dialog in the desktop shell', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      value: {},
      configurable: true,
    })

    const result = await pickFolder()
    // No Tauri host answers in jsdom, so the desktop branch reports the
    // failure rather than silently falling through to the browser one.
    expect(result.kind).toBe('error')
  })
})

describe('normalizeWindowsPath', () => {
  it('removes Windows device path prefixes', () => {
    expect(normalizeWindowsPath('\\\\?\\D:\\video\\orca')).toBe('D:\\video\\orca')
    expect(normalizeWindowsPath('\\\\?\\UNC\\server\\share\\orca')).toBe(
      '\\\\server\\share\\orca',
    )
    expect(normalizeWindowsPath('D:\\video\\orca')).toBe('D:\\video\\orca')
  })
})
