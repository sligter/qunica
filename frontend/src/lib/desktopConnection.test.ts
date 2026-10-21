import { afterEach, expect, it } from 'vitest'
import { readDesktopInstances, saveDesktopConnection, useDesktopConnection } from './desktopConnection'

afterEach(() => {
  localStorage.clear()
  useDesktopConnection.setState({ connection: { mode: 'local' }, instances: [{ mode: 'local' }] })
})

it('keeps multiple normalized instances and migrates the previously selected server', () => {
  localStorage.setItem('qunica:desktop-connection:v1', JSON.stringify({ mode: 'remote', origin: 'https://first.example' }))
  expect(readDesktopInstances()).toContainEqual({ mode: 'remote', origin: 'https://first.example' })
  saveDesktopConnection({ mode: 'remote', origin: 'https://second.example' })
  saveDesktopConnection({ mode: 'remote', origin: 'https://first.example/' })
  saveDesktopConnection({ mode: 'local' })
  expect(readDesktopInstances()).toEqual([
    { mode: 'local' }, { mode: 'remote', origin: 'https://first.example' }, { mode: 'remote', origin: 'https://second.example' },
  ])
})

it('excludes invalid saved destinations and duplicate instances', () => {
  localStorage.setItem('qunica:desktop-instances:v1', JSON.stringify([
    { mode: 'remote', origin: 'https://valid.example/' },
    { mode: 'remote', origin: 'https://valid.example' },
    { mode: 'remote', origin: 'https://user:secret@bad.example/' },
    { mode: 'remote', origin: 'https://bad.example/path' },
    { mode: 'remote', origin: 'javascript:alert(1)' }, null,
  ]))
  expect(readDesktopInstances()).toEqual([{ mode: 'local' }, { mode: 'remote', origin: 'https://valid.example' }])
})
