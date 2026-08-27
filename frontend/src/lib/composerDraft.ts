const STORAGE_PREFIX = 'qunica:composer-draft:v1:'

export function readComposerDraft(id?: string): string {
  if (!id) return ''
  try {
    return localStorage.getItem(`${STORAGE_PREFIX}${id}`) ?? ''
  } catch {
    return ''
  }
}

export function writeComposerDraft(id: string | undefined, value: string): void {
  if (!id) return
  try {
    const key = `${STORAGE_PREFIX}${id}`
    if (value) localStorage.setItem(key, value)
    else localStorage.removeItem(key)
  } catch {
    // Draft persistence must never block typing or sending.
  }
}

export function clearComposerDrafts(): void {
  try {
    for (let index = localStorage.length - 1; index >= 0; index -= 1) {
      const key = localStorage.key(index)
      if (key?.startsWith(STORAGE_PREFIX)) localStorage.removeItem(key)
    }
  } catch {
    // Storage may be unavailable in restricted browser contexts.
  }
}
