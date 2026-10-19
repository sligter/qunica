/** The user-selected instance is the only external API/WS destination in a native shell. */
export function applyConnectionPolicy(origin: string): void {
  const url = new URL(origin)
  const websocket = new URL(origin)
  websocket.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
  const development = import.meta.env.DEV ? ' http://localhost:5173 ws://localhost:5173 http://127.0.0.1:5173 ws://127.0.0.1:5173' : ''
  const policy = document.createElement('meta')
  policy.httpEquiv = 'Content-Security-Policy'
  policy.content = `connect-src 'self' ipc: http://ipc.localhost https://ipc.localhost ${url.origin} ${websocket.origin}${development}`
  // Policies only become stricter during a document's lifetime. Instance changes
  // reload the shell after clearing credentials, which installs the new origin.
  document.head.appendChild(policy)
}
