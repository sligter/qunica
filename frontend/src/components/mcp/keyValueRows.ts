/** One editable row. Rows carry their own id so React keys survive reordering. */
export interface KeyValueRow {
  id: string
  key: string
  value: string
}

let nextRowId = 0

/** A blank row, ready to append. */
export function emptyRow(): KeyValueRow {
  nextRowId += 1
  return { id: `row-${nextRowId}`, key: '', value: '' }
}

/** Turn a record into editor rows, preserving key order. */
export function rowsFromRecord(record: Record<string, string> | undefined): KeyValueRow[] {
  return Object.entries(record ?? {}).map(([key, value]) => ({ ...emptyRow(), key, value }))
}

/**
 * Collapse editor rows back into a record, dropping rows with a blank key.
 *
 * A blank key is always an unfinished row rather than a real entry, and sending
 * one would be rejected by the server anyway.
 */
export function recordFromRows(rows: KeyValueRow[]): Record<string, string> {
  const record: Record<string, string> = {}
  for (const row of rows) {
    const key = row.key.trim()
    if (key) record[key] = row.value
  }
  return record
}
