import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import type { Plugin } from 'vite'

/** Exact build output names, never URL extensions: workspace files are private. */
export function serviceWorkerSource(files: string[], version: string): string {
  return `const CACHE = ${JSON.stringify(`qunica-static-${version}`)};
const FILES = new Set(${JSON.stringify(files)});
self.addEventListener('install', event => {
  event.waitUntil(caches.open(CACHE));
});
// Let the browser activate only after old tabs close. No forced reload or skipWaiting.
self.addEventListener('activate', event => {
  event.waitUntil(caches.keys().then(keys => Promise.all(keys
    .filter(key => key.startsWith('qunica-static-') && key !== CACHE)
    .map(key => caches.delete(key)))));
});
self.addEventListener('fetch', event => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== 'GET' || request.headers.has('authorization') ||
      request.mode === 'navigate' || url.origin !== self.location.origin ||
      url.search || !FILES.has(url.pathname)) return;
  event.respondWith(caches.open(CACHE).then(async cache => {
    const cached = await cache.match(request);
    if (cached) return cached;
    const response = await fetch(request);
    if (response.ok && !response.redirected && response.type !== 'opaque') {
      await cache.put(request, response.clone());
    }
    return response;
  }));
});
`
}

export function pwaAssets(): Plugin {
  let publicDir = ''
  return {
    name: 'qunica-pwa',
    apply: 'build',
    configResolved(config) { publicDir = config.publicDir },
    generateBundle(_options, bundle) {
      const files = Object.keys(bundle)
        .filter(name => name.startsWith('assets/') && !name.endsWith('.map'))
        .map(name => `/${name}`)
      files.push('/icons/180x180.png', '/icons/192x192.png', '/icons/512x512.png')
      const hash = createHash('sha256').update(JSON.stringify(files))
      for (const size of [180, 192, 512]) hash.update(readFileSync(`${publicDir}/icons/${size}x${size}.png`))
      const version = hash.digest('hex').slice(0, 16)
      this.emitFile({ type: 'asset', fileName: 'sw.js', source: serviceWorkerSource(files, version) })
    },
  }
}
