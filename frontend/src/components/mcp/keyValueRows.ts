/** One editable row. Rows carry their own id so React keys survive reordering. */
export interface KeyValueRow {
  id: string
  key: string
  value: string
  /**
   * Whether the operator has typed into this row's value since it was loaded.
   *
   * Secret values (HTTP headers) come back from the API masked, so an untouched
   * row's `value` is a placeholder rather than the real credential. Sending it
   * back would overwrite the stored secret with the mask — or, since the row is
   * seeded blank, with an empty string. `dirty` is what lets the payload say
   * "keep whatever is stored" for a row nobody edited.
   */
  dirty: boolean
}

let nextRowId = 0

/** A blank row, ready to append. A new row is dirty by definition. */
export function emptyRow(): KeyValueRow {
  nextRowId += 1
  return { id: `row-${nextRowId}`, key: '', value: '', dirty: true }
}

/**
 * Turn a record into editor rows, preserving key order.
 *
 * Used for values that are NOT secret (stdio env vars), where the real value
 * round-trips through the API and so starts out clean and complete.
 */
export function rowsFromRecord(record: Record<string, string> | undefined): KeyValueRow[] {
  return Object.entries(record ?? {}).map(([key, value]) => ({
    ...emptyRow(),
    key,
    value,
    dirty: false,
  }))
}

/**
 * Turn a masked record into editor rows whose values start empty.
 *
 * The caller only knows the keys and a mask, so each row renders blank and
 * serializes as "keep" until the operator types a replacement.
 */
export function maskedRowsFromRecord(
  record: Record<string, string> | undefined,
): KeyValueRow[] {
  return Object.keys(record ?? {}).map((key) => ({
    ...emptyRow(),
    key,
    value: '',
    dirty: false,
  }))
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

/**
 * Collapse rows into the keep-or-set map the MCP API expects for secrets.
 *
 * An edited row sends its new value; an untouched row sends `null`, which the
 * server resolves to the stored value. A key the operator deleted is simply
 * absent from the map, which is how the server is told to drop that header —
 * so revoking a credential works, and editing an unrelated field does not
 * silently wipe one.
 */
export function secretRecordFromRows(
  rows: KeyValueRow[],
): Record<string, string | null> {
  const record: Record<string, string | null> = {}
  for (const row of rows) {
    const key = row.key.trim()
    if (!key) continue
    record[key] = row.dirty ? row.value : null
  }
  return record
}
