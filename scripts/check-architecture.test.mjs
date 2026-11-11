import test from 'node:test'
import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { dependencies, functions, report, violations } from './check-architecture.mjs'

const runtime = 'backend-rs/crates/backend/src/runtime/example.rs'
const projection = 'frontend/src/stores/schedulerProjection.ts'

test('Rust function indicators exclude strings, nested comments and preserve lifetimes', () => {
  const source = 'fn visible(x: &str) { let s = r###"if { fn hidden() {} }"###; /* if /* fn nope() {} */ */ if true {} }\n'
    + "fn borrowed<'a>(x: &'a str) -> &'a str { x }"
  assert.deepEqual(functions('a.rs', source).map((fn) => [fn.name, fn.branchTokens]), [['visible', 1], ['borrowed', 0]])
})

test('TypeScript compiler finds Store arrow functions and ignores strings in branch indicators', () => {
  const result = functions('store.ts', "const state = { apply: (s: string) => { const label='if'; if (s) return s; return label } }")
  assert.equal(result[0].name, 'apply')
  assert.equal(result[0].branchTokens, 1)
})

test('new grouped and aliased reverse business imports fail, existing debt and ApiError remain allowed', () => {
  const grouped = 'use crate::{api::{groups as g, error::ApiError}}; fn run() { g::read(); }'
  assert.deepEqual(violations(runtime, '', grouped), ['crate::api::groups'])
  assert.deepEqual(violations('backend-rs/crates/backend/src/group_notes.rs', '', grouped), ['crate::api::groups'])
  assert.deepEqual(violations(runtime, '', 'use crate::api::approvals::Request;'), ['crate::api::approvals'])
  assert.deepEqual(violations(runtime, '', 'use crate::api as web; fn run() { web::commands::read(); }'), ['crate::api::commands'])
  assert.deepEqual(violations(runtime, grouped, grouped + '\nfn second() {}'), [])
  assert.deepEqual(violations(runtime, '', 'use crate::api::error::ApiError;'), [])
  assert.deepEqual(violations(runtime, '', '// crate::api::groups\nfn run(){let text="crate::api::commands";}'), [])
})

test('projection may import Store types but may not load Store or Zustand at runtime', () => {
  assert.deepEqual(violations(projection, '', "import type {StreamRun} from './messageStore'"), [])
  assert.deepEqual(violations(projection, '', "import {type StreamRun} from './messageStore'"), [])
  assert.deepEqual(violations(projection, '', "import {useMessageStore} from './messageStore'"), ['runtime:./messageStore'])
  assert.deepEqual(violations(projection, '', "import {create} from 'zustand'"), ['runtime:zustand'])
  assert.deepEqual(dependencies('x.ts', "import type { X } from './types'"), ['type:./types'])
})

test('real Git diff reports touched functions and CLI returns nonzero for an added violation', () => {
  const root = fileURLToPath(new URL('../', import.meta.url))
  const tempRoot = resolve(root, '.agents', 'architecture-tests')
  mkdirSync(tempRoot, { recursive: true })
  const repo = mkdtempSync(join(tempRoot, 'fixture-'))
  const git = (...args) => execFileSync('git', args, { cwd: repo, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] })
  try {
    git('init', '-q')
    mkdirSync(join(repo, dirname(runtime)), { recursive: true })
    writeFileSync(join(repo, runtime), 'fn one() { }\n\nfn two() { }\n')
    git('add', '.')
    git('-c', 'user.name=Architecture Test', '-c', 'user.email=architecture@example.test', 'commit', '-qm', 'baseline')
    const base = git('rev-parse', 'HEAD').trim()
    writeFileSync(join(repo, runtime), 'fn one() { if true { } }\n\nfn two() { }\n')
    const first = report(base, repo)
    assert.deepEqual(first.files[0].after.map((fn) => fn.name), ['one'])
    assert.equal(first.files[0].after[0].branchTokens, 1)
    const bad = join(repo, 'backend-rs/crates/backend/src/runtime/new.rs')
    writeFileSync(bad, 'use crate::api::groups; fn run() {}')
    const script = fileURLToPath(new URL('./check-architecture.mjs', import.meta.url))
    const failed = spawnSync(process.execPath, [script, '--base', base, '--repo', repo], { encoding: 'utf8' })
    assert.equal(failed.status, 1, failed.stderr)
    assert.equal(JSON.parse(failed.stdout).violationCount, 1)
    rmSync(bad)
    const passed = spawnSync(process.execPath, [script, '--base', base, '--repo', repo], { encoding: 'utf8' })
    assert.equal(passed.status, 0, passed.stderr)
    assert.equal(JSON.parse(passed.stdout).violationCount, 0)
    assert.throws(() => report('--help', repo), /commit SHA/)
    assert.match(report('0'.repeat(40), repo).baselineReason, /existing-debt baseline/)
  } finally {
    assert.equal(dirname(resolve(repo)), tempRoot)
    rmSync(repo, { recursive: true, force: true })
  }
})
