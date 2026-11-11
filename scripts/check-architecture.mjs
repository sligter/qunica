import { execFileSync } from 'node:child_process'
import { existsSync, lstatSync, readFileSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const require = createRequire(new URL('../frontend/package.json', import.meta.url))
const ts = require('typescript')

// Positions/newlines survive masking. Rust figures are lexical review indicators.
export function mask(source, rust = true) {
  const out = source.split('')
  const blank = (a, b) => { for (let n = a; n < b; n++) if (out[n] !== '\n') out[n] = ' ' }
  for (let i = 0; i < source.length;) {
    const start = i
    if (source.startsWith('//', i)) {
      i = source.indexOf('\n', i); if (i < 0) i = source.length
    } else if (source.startsWith('/*', i)) {
      i += 2
      let depth = 1
      while (i < source.length && depth) {
        if (source.startsWith('/*', i)) { depth++; i += 2 }
        else if (source.startsWith('*/', i)) { depth--; i += 2 }
        else i++
      }
    } else {
      const raw = /^(?:br|r)(#*)"/.exec(source.slice(i))
      if (raw) {
        const end = source.indexOf('"' + raw[1], i + raw[0].length)
        i = end < 0 ? source.length : end + 1 + raw[1].length
      } else if (source[i] === '"' || source.charCodeAt(i) === 96 || (source[i] === "'" && (!rust || /^'(?:\\.|[^'\\\n])'/.test(source.slice(i))))) {
        const quote = source[i++]
        while (i < source.length) {
          if (source[i] === '\\') i += 2
          else if (source[i++] === quote) break
        }
      } else { i++; continue }
    }
    blank(start, i)
  }
  return out.join('')
}

export function functions(file, source) {
  const result = [], text = mask(source, file.endsWith('.rs'))
  const line = (offset) => source.slice(0, offset).split('\n').length
  const add = (name, start, end) => result.push({ name, start: line(start), end: line(end),
    branchTokens: (text.slice(start, end).match(/\b(?:if|match|for|while|loop|case|catch)\b/g) ?? []).length })
  if (file.endsWith('.rs')) {
    for (const m of text.matchAll(/\bfn\s+([A-Za-z_]\w*)\s*[<(]/g)) {
      const open = text.indexOf('{', m.index), semi = text.indexOf(';', m.index)
      if (open < 0 || (semi >= 0 && semi < open)) continue
      let depth = 1, end = open + 1
      for (; end < text.length && depth; end++) {
        if (text[end] === '{') depth++
        if (text[end] === '}') depth--
      }
      add(m[1], m.index, end)
    }
  } else {
    const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true)
    const visit = (node) => {
      if (ts.isFunctionDeclaration(node) || ts.isMethodDeclaration(node) || ts.isArrowFunction(node) || ts.isFunctionExpression(node)) {
        add(node.name?.getText(tree) ?? node.parent?.name?.getText(tree) ?? '<anonymous>', node.getStart(tree), node.end)
      }
      ts.forEachChild(node, visit)
    }
    visit(tree)
  }
  return result
}

function splitTop(text) {
  let depth = 0, start = 0
  const result = []
  for (let i = 0; i < text.length; i++) {
    if (text[i] === '{') depth++
    if (text[i] === '}') depth--
    if (text[i] === ',' && depth === 0) { result.push(text.slice(start, i)); start = i + 1 }
  }
  result.push(text.slice(start))
  return result
}

function uses(text, prefix = '') {
  return splitTop(text).flatMap((part) => {
    part = part.trim()
    const brace = part.indexOf('{')
    if (brace >= 0) return uses(part.slice(brace + 1, part.lastIndexOf('}')), prefix + part.slice(0, brace).replace(/\s/g, ''))
    const [path, alias] = part.split(/\s+as\s+/)
    const full = (prefix + path.replace(/\s/g, '')).replace(/::self$/, '')
    return full ? [{ path: full, alias: alias?.trim() ?? full.split('::').at(-1) }] : []
  })
}

export function dependencies(file, source) {
  if (file.endsWith('.rs')) {
    const text = mask(source)
    const imports = [...text.matchAll(/\buse\s+([^;]+);/g)].flatMap((m) => uses(m[1]))
    const paths = imports.map((entry) => entry.path)
    paths.push(...[...text.matchAll(/\bcrate::api::([A-Za-z_]\w*)/g)].map((m) => 'crate::api::' + m[1]))
    for (const entry of imports.filter((entry) => entry.path === 'crate::api')) {
      const pattern = new RegExp('\\b' + entry.alias + '::([A-Za-z_]\\w*)', 'g')
      paths.push(...[...text.matchAll(pattern)].map((m) => 'crate::api::' + m[1]))
    }
    return [...new Set(paths.map((path) => path.startsWith('crate::api::') ? path.split('::').slice(0, 3).join('::') : path))].sort()
  }
  const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true)
  return tree.statements.filter(ts.isImportDeclaration).map((node) => {
    const clause = node.importClause, named = clause?.namedBindings
    const onlyTypes = clause?.isTypeOnly || (!clause?.name && named && ts.isNamedImports(named) && named.elements.length > 0 && named.elements.every((entry) => entry.isTypeOnly))
    return (onlyTypes ? 'type:' : 'runtime:') + node.moduleSpecifier.text
  }).sort()
}

export function violations(file, before, after) {
  const old = new Set(dependencies(file, before))
  const added = dependencies(file, after).filter((dep) => !old.has(dep))
  const lower = /^backend-rs\/crates\/backend\/src\/(?:runtime|tools|db|group_notes|acp|llm)(?:\/|\.rs$)/
  return added.filter((dep) =>
    (lower.test(file) && dep.startsWith('crate::api::') && dep !== 'crate::api::error') ||
    (file === 'frontend/src/stores/schedulerProjection.ts' && /^runtime:(?:zustand|react|.*messageStore)/.test(dep)),
  )
}

function ranges(diff, side) {
  return [...diff.matchAll(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/gm)].map((m) => {
    const start = Number(m[side === 'before' ? 1 : 3])
    const count = Number(m[side === 'before' ? 2 : 4] ?? 1)
    return [start, start + Math.max(0, count - 1)]
  })
}

export function report(base, repoRoot = root) {
  const repoGit = (...args) => execFileSync('git', args, { cwd: repoRoot, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, stdio: ['pipe', 'pipe', 'pipe'] })
  const initial = /^0+$/.test(base)
  if (initial) base = repoGit('rev-parse', 'HEAD').trim()
  if (!/^[a-f0-9]{7,40}$/i.test(base)) throw new Error('--base must be a commit SHA')
  repoGit('cat-file', '-e', base + '^{commit}')
  const baseFiles = new Set(repoGit('ls-tree', '-r', '--name-only', '-z', base).split('\0').filter(Boolean))
  const names = new Set(repoGit('diff', '--name-only', '-z', '--no-renames', base, '--').split('\0').filter(Boolean))
  for (const name of repoGit('ls-files', '--others', '--exclude-standard', '-z').split('\0').filter(Boolean)) names.add(name)
  const files = []
  for (const file of [...names].sort()) {
    if (!/^(?:backend-rs\/.*\.rs|frontend\/src\/.*\.tsx?)$/.test(file)) continue
    const before = baseFiles.has(file) ? repoGit('show', base + ':' + file) : ''
    const path = resolve(repoRoot, file)
    if (existsSync(path) && lstatSync(path).isSymbolicLink()) throw new Error('Source symlink requires an explicit review: ' + file)
    const after = existsSync(path) ? readFileSync(path, 'utf8') : ''
    const diff = repoGit('diff', '--no-ext-diff', '--no-textconv', '--unified=0', '--no-renames', base, '--', file)
    const touched = (side, text) => {
      const spans = before === '' || after === '' ? [[1, Number.MAX_SAFE_INTEGER]] : ranges(diff, side)
      return functions(file, text).filter((fn) => spans.some(([start, end]) => fn.start <= end && fn.end >= start))
    }
    files.push({ file, before: touched('before', before), after: touched('after', after),
      importsAdded: dependencies(file, after).filter((dep) => !dependencies(file, before).includes(dep)),
      importsRemoved: dependencies(file, before).filter((dep) => !dependencies(file, after).includes(dep)),
      violations: violations(file, before, after) })
  }
  return { base, baselineReason: initial ? 'Initial push establishes the existing-debt baseline.' : null,
    metrics: 'Function spans and branch-token counts are review indicators. Rust parsing is lexical; moves may appear as added/deleted functions. No whole-repository complexity threshold.',
    files, violationCount: files.reduce((sum, file) => sum + file.violations.length, 0) }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2)
    const repoRoot = args.includes('--repo') ? resolve(args[args.indexOf('--repo') + 1]) : root
    const base = args.includes('--base') ? args[args.indexOf('--base') + 1] : execFileSync('git', ['rev-parse', 'HEAD^'], { cwd: repoRoot, encoding: 'utf8' }).trim()
    const result = report(base, repoRoot), json = JSON.stringify(result, null, 2) + '\n'
    if (args.includes('--output')) writeFileSync(args[args.indexOf('--output') + 1], json)
    console.log(json)
    process.exitCode = result.violationCount ? 1 : 0
  } catch (error) { console.error(error.message); process.exitCode = 1 }
}
