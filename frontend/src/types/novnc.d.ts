declare module '@novnc/novnc/lib/rfb' {
  export default class RFB extends EventTarget {
    constructor(target: HTMLElement, url: string | WebSocket)
    viewOnly: boolean
    scaleViewport: boolean
    resizeSession: boolean
    disconnect(): void
  }
}
