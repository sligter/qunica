import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/**
 * Readable text for a thrown value.
 *
 * `String(err)` on an Error yields "Error: socket hang up", so templates that
 * interpolates it into a sentence end up reading "Could not load: Error: socket
 * hang up". The instance check drops the redundant class name.
 */
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
