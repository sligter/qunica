# Bilingual Internationalization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Translate the complete AG Swarmer frontend between `zh-CN` and `en-US`, with an account-level language preference and locale-aware formatting.

**Architecture:** `i18next` owns semantic translation keys and namespace resources; a small bootstrap module chooses the pre-auth locale, while `useApplyLanguage` reconciles it with authenticated system settings. The Rust settings API persists the only two supported locale values, and shared formatting/error helpers keep locale-sensitive behavior out of individual components.

**Tech Stack:** React 19, TypeScript 5.7, i18next, react-i18next, TanStack Query, Vitest/Testing Library, Rust/Axum, SQLx/SQLite.

## Global Constraints

- Supported locales are exactly `zh-CN` and `en-US`.
- Language is an account-level preference and the authenticated server value is authoritative.
- Translate all frontend copy, accessibility labels, frontend validation and known error messages, page titles, empty/loading/error states, dates, relative times, and number formatting.
- Do not translate Agent-generated content or arbitrary backend diagnostics.
- Production missing-key behavior falls back to `en-US` and never exposes a raw key.
- Existing group-chat behavior must not change.

## File Structure

- Create `backend-rs/crates/backend/src/db/migrations/0004_system_settings_language.sql`: account-language schema migration.
- Modify `backend-rs/crates/backend/src/api/system_settings.rs`: read, validate, persist, and return language.
- Modify `backend-rs/crates/backend/tests/providers_settings.rs`: settings migration/default/validation/account-isolation coverage.
- Modify `frontend/package.json` and `pnpm-lock.yaml`: add `i18next` and `react-i18next`.
- Create `frontend/src/i18n/resources/en-US.ts`: English resources grouped by namespace.
- Create `frontend/src/i18n/resources/zh-CN.ts`: Chinese resources with key parity.
- Create `frontend/src/i18n/index.ts`: i18next initialization and supported-locale helpers.
- Create `frontend/src/i18n/index.test.ts`: bootstrap and resource-parity tests.
- Create `frontend/src/hooks/useApplyLanguage.ts`: server reconciliation, `<html lang>`, and local mirror.
- Create `frontend/src/hooks/useApplyLanguage.test.tsx`: authenticated preference and rollback behavior.
- Create `frontend/src/lib/format.ts` and `frontend/src/lib/format.test.ts`: date, time, relative-time, and numeric formatting.
- Create `frontend/src/lib/localizedApiError.ts` and `frontend/src/lib/localizedApiError.test.ts`: known API error-code mapping.
- Modify `frontend/src/types/api.ts`: add the `Language` contract.
- Modify `frontend/src/main.tsx` and `frontend/src/App.tsx`: initialize and apply language.
- Modify all user-facing `.tsx` files under `frontend/src/components` and `frontend/src/pages`: replace literal UI copy with semantic translation keys.

---

### Task 1: Add the i18n runtime and deterministic bootstrap

**Files:**
- Modify: `frontend/package.json`
- Modify: `pnpm-lock.yaml`
- Create: `frontend/src/i18n/resources/en-US.ts`
- Create: `frontend/src/i18n/resources/zh-CN.ts`
- Create: `frontend/src/i18n/index.ts`
- Create: `frontend/src/i18n/index.test.ts`
- Modify: `frontend/src/main.tsx`

**Interfaces:**
- Produces: `type Language = 'zh-CN' | 'en-US'` from `@/i18n` until Task 2 re-exports the API type.
- Produces: `SUPPORTED_LANGUAGES`, `LANGUAGE_MIRROR_KEY`, `normalizeLanguage(value)`, `detectBootstrapLanguage()`, `readLanguageMirror()`, `writeLanguageMirror(language)`, and initialized default export `i18n`.
- Produces: translation namespaces `common`, `auth`, `navigation`, `chat`, `agents`, `groups`, `providers`, `skills`, `workspaces`, and `settings`.

- [ ] **Step 1: Install the runtime dependencies**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend add i18next react-i18next
```

Expected: `frontend/package.json` lists both packages under `dependencies` and `pnpm-lock.yaml` changes.

- [ ] **Step 2: Write failing bootstrap and resource-parity tests**

Create `frontend/src/i18n/index.test.ts` with tests that reset `localStorage`, override `navigator.language`, and assert:

```ts
import { afterEach, describe, expect, it, vi } from 'vitest'

import { enUS } from './resources/en-US'
import { zhCN } from './resources/zh-CN'
import {
  LANGUAGE_MIRROR_KEY,
  detectBootstrapLanguage,
  normalizeLanguage,
} from './index'

function keys(value: unknown, prefix = ''): string[] {
  if (typeof value !== 'object' || value === null) return [prefix]
  return Object.entries(value).flatMap(([key, child]) =>
    keys(child, prefix ? `${prefix}.${key}` : key),
  )
}

describe('i18n bootstrap', () => {
  afterEach(() => {
    localStorage.clear()
    vi.unstubAllGlobals()
  })

  it('keeps the two supported locale values only', () => {
    expect(normalizeLanguage('zh-CN')).toBe('zh-CN')
    expect(normalizeLanguage('en-US')).toBe('en-US')
    expect(normalizeLanguage('fr-FR')).toBeNull()
  })

  it('prefers the local mirror before browser detection', () => {
    localStorage.setItem(LANGUAGE_MIRROR_KEY, 'en-US')
    vi.stubGlobal('navigator', { language: 'zh-CN' })
    expect(detectBootstrapLanguage()).toBe('en-US')
  })

  it('maps Chinese browser locales to zh-CN and everything else to en-US', () => {
    vi.stubGlobal('navigator', { language: 'zh-TW' })
    expect(detectBootstrapLanguage()).toBe('zh-CN')
    vi.stubGlobal('navigator', { language: 'de-DE' })
    expect(detectBootstrapLanguage()).toBe('en-US')
  })

  it('keeps English and Chinese resource keys identical', () => {
    expect(keys(zhCN).sort()).toEqual(keys(enUS).sort())
  })
})
```

- [ ] **Step 3: Run the tests to verify the module is missing**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/i18n/index.test.ts
```

Expected: FAIL because `./index`, `./resources/en-US`, and `./resources/zh-CN` do not exist.

- [ ] **Step 4: Create the initialized i18n module and initial namespace resources**

Create `frontend/src/i18n/index.ts` with this public contract:

```ts
import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'

import { enUS } from './resources/en-US'
import { zhCN } from './resources/zh-CN'

export type Language = 'zh-CN' | 'en-US'
export const SUPPORTED_LANGUAGES: readonly Language[] = ['zh-CN', 'en-US']
export const LANGUAGE_MIRROR_KEY = 'ag-swarmer:language'

export function normalizeLanguage(value: unknown): Language | null {
  return value === 'zh-CN' || value === 'en-US' ? value : null
}

export function readLanguageMirror(): Language | null {
  try {
    return normalizeLanguage(localStorage.getItem(LANGUAGE_MIRROR_KEY))
  } catch {
    return null
  }
}

export function writeLanguageMirror(language: Language): void {
  try {
    localStorage.setItem(LANGUAGE_MIRROR_KEY, language)
  } catch {
    // Persistence failure must not block rendering.
  }
}

export function detectBootstrapLanguage(): Language {
  const mirrored = readLanguageMirror()
  if (mirrored) return mirrored
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US'
}

void i18n.use(initReactI18next).init({
  lng: detectBootstrapLanguage(),
  fallbackLng: 'en-US',
  supportedLngs: SUPPORTED_LANGUAGES,
  defaultNS: 'common',
  ns: Object.keys(enUS),
  resources: {
    'en-US': enUS,
    'zh-CN': zhCN,
  },
  interpolation: { escapeValue: false },
  returnNull: false,
  saveMissing: import.meta.env.DEV,
  missingKeyHandler: import.meta.env.DEV
    ? (_languages, namespace, key) => console.warn(`Missing i18n key: ${namespace}:${key}`)
    : undefined,
})

export default i18n
```

Create both resource files with all ten namespace roots and the initial keys below; later tasks extend the same objects:

```ts
export const enUS = {
  common: {
    actions: { save: 'Save', cancel: 'Cancel', clear: 'Clear', delete: 'Delete', retry: 'Retry' },
    state: { loading: 'Loading…', unavailable: 'Unavailable' },
  },
  auth: {}, navigation: {}, chat: {}, agents: {}, groups: {}, providers: {}, skills: {}, workspaces: {}, settings: {},
} as const
```

```ts
import type { enUS } from './en-US'

type TranslationShape<T> = { [K in keyof T]: T[K] extends string ? string : TranslationShape<T[K]> }

export const zhCN: TranslationShape<typeof enUS> = {
  common: {
    actions: { save: '保存', cancel: '取消', clear: '清除', delete: '删除', retry: '重试' },
    state: { loading: '加载中…', unavailable: '不可用' },
  },
  auth: {}, navigation: {}, chat: {}, agents: {}, groups: {}, providers: {}, skills: {}, workspaces: {}, settings: {},
}
```

Import `@/i18n` in `frontend/src/main.tsx` before rendering React.

- [ ] **Step 5: Run the bootstrap tests**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/i18n/index.test.ts
```

Expected: PASS with four tests.

- [ ] **Step 6: Commit the i18n foundation**

```powershell
git add frontend/package.json pnpm-lock.yaml frontend/src/i18n frontend/src/main.tsx
git commit -m "feat(frontend): add bilingual i18n foundation"
```

---

### Task 2: Persist language in account settings

**Files:**
- Create: `backend-rs/crates/backend/src/db/migrations/0004_system_settings_language.sql`
- Modify: `backend-rs/crates/backend/src/api/system_settings.rs`
- Modify: `backend-rs/crates/backend/tests/providers_settings.rs`
- Modify: `frontend/src/types/api.ts`

**Interfaces:**
- Consumes: `Language` values `zh-CN | en-US` from Task 1.
- Produces: settings JSON field `language: 'zh-CN' | 'en-US'`.
- Produces: `SystemSettingsUpdate.language?: Language | null`; JSON `null` normalizes to `en-US`.

- [ ] **Step 1: Add failing backend settings tests**

Extend `backend-rs/crates/backend/tests/providers_settings.rs` with assertions that a fresh account returns `"language": "en-US"`, PATCH with `{"language":"zh-CN"}` persists across GET, a second account retains `en-US`, and `{"language":"fr-FR"}` returns HTTP 422 with code `invalid_input`.

Use the existing `spawn_app`, `register_user`, and authenticated request helpers in that file; keep the concrete assertions:

```rust
assert_eq!(created["language"], "en-US");
assert_eq!(updated["language"], "zh-CN");
assert_eq!(reloaded["language"], "zh-CN");
assert_eq!(other_account["language"], "en-US");
assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
assert_eq!(invalid_json["error"]["code"], "invalid_input");
```

- [ ] **Step 2: Run the backend test to verify it fails**

Run:

```powershell
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend --test providers_settings system_settings -- --nocapture
```

Expected: FAIL because the response has no `language` field.

- [ ] **Step 3: Add the migration and settings contract**

Create migration:

```sql
ALTER TABLE system_settings
ADD COLUMN language TEXT NOT NULL DEFAULT 'en-US';
```

In `system_settings.rs`, add `DEFAULT_LANGUAGE`, include `language` in `SETTINGS_COLUMNS`, `UpdateRequest`, `SettingsResponse`, `SettingsRow`, conversion, insert, update, and bind order. Implement:

```rust
fn normalize_language(raw: Option<&str>) -> Result<String, ApiError> {
    let language = raw.map(str::trim).filter(|value| !value.is_empty()).unwrap_or(DEFAULT_LANGUAGE);
    match language {
        "zh-CN" | "en-US" => Ok(language.to_string()),
        _ => Err(ApiError::invalid_input("language must be 'zh-CN' or 'en-US'")),
    }
}
```

In `frontend/src/types/api.ts`, add:

```ts
export type Language = 'zh-CN' | 'en-US'
```

and add `language: Language` to `SystemSettingsRead` plus `language?: Language | null` to `SystemSettingsUpdate`. Change Task 1's `i18n/index.ts` to import and re-export this type rather than declaring a duplicate.

- [ ] **Step 4: Run focused backend and frontend type checks**

Run:

```powershell
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend --test providers_settings system_settings -- --nocapture
pnpm --filter @ag-swarmer/frontend type-check
```

Expected: both commands PASS.

- [ ] **Step 5: Commit account language persistence**

```powershell
git add backend-rs/crates/backend/src/db/migrations/0004_system_settings_language.sql backend-rs/crates/backend/src/api/system_settings.rs backend-rs/crates/backend/tests/providers_settings.rs frontend/src/types/api.ts frontend/src/i18n/index.ts
git commit -m "feat(settings): persist account language"
```

---

### Task 3: Reconcile server language and centralize locale formatting

**Files:**
- Create: `frontend/src/hooks/useApplyLanguage.ts`
- Create: `frontend/src/hooks/useApplyLanguage.test.tsx`
- Create: `frontend/src/lib/format.ts`
- Create: `frontend/src/lib/format.test.ts`
- Modify: `frontend/src/App.tsx`

**Interfaces:**
- Consumes: `useSystemSettings()` and `Language` from Task 2.
- Produces: `useApplyLanguage(): void`.
- Produces: `formatDateTime(value, language)`, `formatTime(value, language)`, `formatNumber(value, language)`, and `formatRelativeTime(value, language, now?)`.

- [ ] **Step 1: Write failing hook and formatter tests**

Test that `useApplyLanguage` changes i18next, sets `document.documentElement.lang`, and writes the mirror when mocked settings return `zh-CN`. Test formatter outputs with fixed values:

```ts
expect(formatNumber(12345, 'en-US')).toBe('12,345')
expect(formatNumber(12345, 'zh-CN')).toBe('12,345')
expect(formatRelativeTime('2026-07-18T11:59:00Z', 'en-US', new Date('2026-07-18T12:00:00Z'))).toBe('1m ago')
expect(formatRelativeTime('2026-07-18T11:59:00Z', 'zh-CN', new Date('2026-07-18T12:00:00Z'))).toBe('1分钟前')
```

- [ ] **Step 2: Run focused tests to verify they fail**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/hooks/useApplyLanguage.test.tsx src/lib/format.test.ts
```

Expected: FAIL because the modules do not exist.

- [ ] **Step 3: Implement the hook and locale helpers**

`useApplyLanguage` reads the authenticated settings query, calls `i18n.changeLanguage(serverLanguage)` only when needed, sets the HTML `lang`, and mirrors only a server-confirmed value. `formatRelativeTime` uses `Intl.RelativeTimeFormat(language, { numeric: 'auto', style: 'narrow' })` with second/minute/hour/day buckets; `formatDateTime`, `formatTime`, and `formatNumber` use the matching `Intl` formatter with the explicit language argument.

Add `useApplyLanguage()` next to `useApplyAppearance()` in `App.tsx`.

- [ ] **Step 4: Run focused tests and type-check**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/hooks/useApplyLanguage.test.tsx src/lib/format.test.ts
pnpm --filter @ag-swarmer/frontend type-check
```

Expected: PASS.

- [ ] **Step 5: Commit language reconciliation and formatting**

```powershell
git add frontend/src/hooks/useApplyLanguage.ts frontend/src/hooks/useApplyLanguage.test.tsx frontend/src/lib/format.ts frontend/src/lib/format.test.ts frontend/src/App.tsx
git commit -m "feat(frontend): apply account locale"
```

---

### Task 4: Translate authentication, navigation, home, and settings

**Files:**
- Modify: `frontend/src/i18n/resources/en-US.ts`
- Modify: `frontend/src/i18n/resources/zh-CN.ts`
- Modify: `frontend/src/components/auth/AuthForm.tsx`
- Modify: `frontend/src/components/layout/AppSidebar.tsx`
- Modify: `frontend/src/components/layout/AgentsListColumn.tsx`
- Modify: `frontend/src/components/layout/ProvidersListColumn.tsx`
- Modify: `frontend/src/components/layout/SkillsListColumn.tsx`
- Modify: `frontend/src/components/layout/WorkspacesListColumn.tsx`
- Modify: `frontend/src/components/layout/DetailShell.tsx`
- Modify: `frontend/src/components/layout/EntityLayout.tsx`
- Modify: `frontend/src/components/layout/LegacyDetailRedirect.tsx`
- Modify: `frontend/src/components/layout/ListColumn.tsx`
- Modify: `frontend/src/components/layout/SettingsLayout.tsx`
- Modify: `frontend/src/components/layout/VerticalResizeHandle.tsx`
- Modify: `frontend/src/components/ui/confirm-dialog.tsx`
- Modify: `frontend/src/pages/auth/LoginPage.tsx`
- Modify: `frontend/src/pages/auth/RegisterPage.tsx`
- Modify: `frontend/src/pages/home/ChatHomePage.tsx`
- Modify: `frontend/src/pages/settings/SystemSettingsPage.tsx`
- Modify: `frontend/src/pages/NotFoundPage.tsx`
- Modify: `frontend/src/routes.tsx`
- Modify: `frontend/src/components/layout/AppLayout.test.tsx`
- Create: `frontend/src/pages/settings/SystemSettingsPage.test.tsx`

**Interfaces:**
- Consumes: `useTranslation(namespace)`, locale formatters, and settings `language`.
- Produces: optimistic `onLanguageChange(next: Language)` with rollback to the last server-confirmed value.

- [ ] **Step 1: Add failing shell and settings language tests**

Render the sidebar and settings page under an i18next test instance. Assert English labels (`Direct Chats` is added by the direct-chat plan, so this plan asserts `Groups`, `Agents`, `Settings`) and Chinese labels (`群聊`, `Agent`, `设置`). In the settings test, click `中文`, assert immediate Chinese copy and PATCH `{ language: 'zh-CN' }`; reject the mutation and assert rollback to `English` plus localized failure copy.

- [ ] **Step 2: Run the focused component tests to verify failure**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/components/layout/AppLayout.test.tsx src/pages/settings/SystemSettingsPage.test.tsx
```

Expected: FAIL because the components still contain literal English and no language control exists.

- [ ] **Step 3: Add explicit resource keys for these surfaces**

Populate `auth`, `navigation`, `settings`, and the relevant `common` keys. At minimum the resource contract includes:

```ts
auth: {
  login: { title: 'Sign in', submit: 'Sign in', switchPrompt: 'Need an account?', switchAction: 'Register' },
  register: { title: 'Create account', submit: 'Register', switchPrompt: 'Already have an account?', switchAction: 'Sign in' },
  fields: { name: 'Name', email: 'Email', password: 'Password' },
  errors: { invalidCredentials: 'Email or password is incorrect.', network: 'Unable to reach the server.' },
},
navigation: {
  groups: 'Groups', agents: 'Agents', providers: 'Providers', skills: 'Skills', workspaces: 'Workspaces',
  settings: 'Settings', newGroup: 'New group', searchGroups: 'Search groups', library: 'Library',
  expandSidebar: 'Expand sidebar', collapseSidebar: 'Collapse sidebar', userMenu: 'User menu', logout: 'Log out',
},
settings: {
  title: 'System settings', subtitle: 'Account-level preferences and integrations.', appearance: 'Appearance',
  theme: 'Theme', themeDescription: 'Choose the app theme for this account. Saved instantly.',
  light: 'Light', dark: 'Dark', system: 'System', language: 'Language', chinese: '中文', english: 'English',
  languageDescription: 'Choose the interface language for this account. Saved instantly.',
  errors: { appearance: 'Appearance update failed.', language: 'Language update failed.', network: 'Network error.' },
},
```

Add exact Chinese counterparts, including `群聊`, `新建群聊`, `搜索群聊`, `设置`, `退出登录`, `系统设置`, `外观`, `语言`, and localized descriptions/errors. Add keys for every remaining literal in the listed files, including Tavily settings, workspace-root settings, loading, empty, and confirmation text.

- [ ] **Step 4: Replace shell/auth/home/settings literals and add the language control**

Use namespace-local `const { t } = useTranslation('settings')`. Replace module-level label arrays with value-only arrays and translate labels during render. Implement language save with the same optimistic/revert pattern as appearance:

```ts
const onLanguageChange = async (next: Language) => {
  if (next === language || update.isPending) return
  const previous = language
  setLanguage(next)
  setLanguageError(null)
  await i18n.changeLanguage(next)
  try {
    await update.mutateAsync({ language: next })
    writeLanguageMirror(next)
  } catch (err) {
    setLanguage(previous)
    await i18n.changeLanguage(previous)
    writeLanguageMirror(previous)
    setLanguageError(errorMessage(err, t('errors.language')))
  }
}
```

Use `formatRelativeTime` in `AppSidebar` and translation pluralization (`agent_one`, `agent_other`) instead of manual singular checks. Set route/page document titles through translated page components, not hard-coded router metadata.

- [ ] **Step 5: Run tests and the resource-parity check**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/i18n/index.test.ts src/components/layout/AppLayout.test.tsx src/pages/settings/SystemSettingsPage.test.tsx
pnpm --filter @ag-swarmer/frontend type-check
```

Expected: PASS.

- [ ] **Step 6: Commit the translated shell**

```powershell
git add frontend/src/i18n frontend/src/components/auth frontend/src/components/layout frontend/src/components/ui/confirm-dialog.tsx frontend/src/pages/auth frontend/src/pages/home frontend/src/pages/settings frontend/src/pages/NotFoundPage.tsx frontend/src/routes.tsx
git commit -m "feat(frontend): translate app shell and settings"
```

---

### Task 5: Translate resource-management screens and forms

**Files:**
- Modify: `frontend/src/i18n/resources/en-US.ts`
- Modify: `frontend/src/i18n/resources/zh-CN.ts`
- Modify: `frontend/src/components/agents/CreateAgentForm.tsx`
- Modify: `frontend/src/components/agents/EditAgentForm.tsx`
- Modify: `frontend/src/components/agents/ExternalRuntimeFields.tsx`
- Modify: `frontend/src/components/agents/RuntimeCapabilityField.tsx`
- Modify: `frontend/src/components/agents/SystemPromptMentionTextarea.tsx`
- Modify: `frontend/src/components/agents/ThinkingLevelControl.tsx`
- Modify: `frontend/src/components/agents/ToolSelector.tsx`
- Modify: `frontend/src/components/agents/WorkspaceField.tsx`
- Modify: `frontend/src/components/providers/CreateProviderForm.tsx`
- Modify: `frontend/src/components/providers/EditProviderForm.tsx`
- Modify: `frontend/src/components/providers/ReasoningPassbackControl.tsx`
- Modify: `frontend/src/components/skills/ImportSkillForm.tsx`
- Modify: `frontend/src/components/skills/SkillResourcesPanel.tsx`
- Modify: `frontend/src/pages/agents/AgentCreatePage.tsx`
- Modify: `frontend/src/pages/agents/AgentDetailPage.tsx`
- Modify: `frontend/src/pages/agents/AgentsIndexPage.tsx`
- Modify: `frontend/src/pages/providers/ProviderCreatePage.tsx`
- Modify: `frontend/src/pages/providers/ProviderDetailPage.tsx`
- Modify: `frontend/src/pages/providers/ProvidersIndexPage.tsx`
- Modify: `frontend/src/pages/skills/SkillCreatePage.tsx`
- Modify: `frontend/src/pages/skills/SkillDetailPage.tsx`
- Modify: `frontend/src/pages/skills/SkillsIndexPage.tsx`
- Modify: `frontend/src/pages/workspace/WorkspaceCreatePage.tsx`
- Modify: `frontend/src/pages/workspace/WorkspaceDetailPage.tsx`
- Modify: `frontend/src/pages/workspace/WorkspacesIndexPage.tsx`
- Modify: `frontend/src/components/agents/AgentRuntimeCapabilities.test.tsx`
- Modify: `frontend/src/components/agents/RuntimeCapabilityField.test.tsx`
- Modify: `frontend/src/components/agents/SystemPromptMentionTextarea.test.tsx`
- Modify: `frontend/src/components/agents/WorkspaceField.test.tsx`

**Interfaces:**
- Consumes: namespaces `agents`, `providers`, `skills`, `workspaces`, and `common`.
- Produces: no literal user-facing English in the listed resource-management surfaces.

- [ ] **Step 1: Add Chinese rendering assertions to representative form tests**

Extend `WorkspaceField.test.tsx`, `RuntimeCapabilityField.test.tsx`, and `AgentRuntimeCapabilities.test.tsx` to switch i18next to `zh-CN` and assert translated labels, picker actions, capability states, and validation errors. Add page smoke tests for each resource index that assert its English and Chinese empty state.

- [ ] **Step 2: Run the focused tests to verify literal-copy failures**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/components/agents
```

Expected: at least the new Chinese assertions FAIL.

- [ ] **Step 3: Populate domain resource contracts**

For each domain, add keys grouped as `list`, `detail`, `form`, `fields`, `actions`, `states`, `validation`, and `errors`. Preserve technical values such as model IDs, runtime kinds, URLs, file paths, Git refs, Skills IDs, and command text. Translate their labels and explanations only. Use interpolation for names and counts, for example:

```ts
agents: {
  list: { title: 'Agents', empty: 'No agents yet.', search: 'Search agents' },
  actions: { create: 'New Agent', edit: 'Edit Agent', delete: 'Delete Agent' },
  fields: { name: 'Name', description: 'Description', systemPrompt: 'System prompt', runtime: 'Runtime', workspace: 'Workspace' },
  validation: { nameRequired: 'Agent name is required.', providerRequired: 'Choose a provider.' },
  deleteConfirm: 'Delete {{name}}? Existing direct-chat history will remain readable.',
},
```

Add the equivalent Chinese keys and complete keys for every literal in the listed files. Use i18next plural keys for counts and `common.actions.*` for shared verbs.

- [ ] **Step 4: Replace literals domain by domain**

Use `useTranslation(['agents', 'common'])` in Agent components, matching namespace pairs for Providers, Skills, and Workspaces. Move module-level option labels into translation keys or translate them during render. Replace `toLocaleString()` with `formatNumber(value, i18n.resolvedLanguage as Language)` and localized unknown/error fallbacks.

- [ ] **Step 5: Run resource tests, domain tests, lint, and type-check**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/i18n/index.test.ts src/components/agents src/pages/agents src/pages/providers src/pages/skills src/pages/workspace
pnpm --filter @ag-swarmer/frontend lint
pnpm --filter @ag-swarmer/frontend type-check
```

Expected: PASS.

- [ ] **Step 6: Commit translated resource management**

```powershell
git add frontend/src/i18n frontend/src/components/agents frontend/src/components/providers frontend/src/components/skills frontend/src/pages/agents frontend/src/pages/providers frontend/src/pages/skills frontend/src/pages/workspace
git commit -m "feat(frontend): translate resource management"
```

---

### Task 6: Translate groups and the complete chat surface

**Files:**
- Modify: `frontend/src/i18n/resources/en-US.ts`
- Modify: `frontend/src/i18n/resources/zh-CN.ts`
- Modify: `frontend/src/components/chat/AgentActivityBubble.tsx`
- Modify: `frontend/src/components/chat/AgentAvatar.tsx`
- Modify: `frontend/src/components/chat/Composer.tsx`
- Modify: `frontend/src/components/chat/DispatchDag.tsx`
- Modify: `frontend/src/components/chat/GroupNotesPanel.tsx`
- Modify: `frontend/src/components/chat/GroupWorkspacePanel.tsx`
- Modify: `frontend/src/components/chat/HumanInputRequestForm.tsx`
- Modify: `frontend/src/components/chat/InterruptedMessageActions.tsx`
- Modify: `frontend/src/components/chat/MarkdownMessage.tsx`
- Modify: `frontend/src/components/chat/MentionPopover.tsx`
- Modify: `frontend/src/components/chat/MessageActions.tsx`
- Modify: `frontend/src/components/chat/MessageItem.tsx`
- Modify: `frontend/src/components/chat/MessageList.tsx`
- Modify: `frontend/src/components/chat/PersistedTurnDetails.tsx`
- Modify: `frontend/src/components/chat/StreamTimeline.tsx`
- Modify: `frontend/src/components/chat/TurnSummary.tsx`
- Modify: `frontend/src/components/chat/TurnTraceDrawer.tsx`
- Modify: `frontend/src/components/chat/WorkspaceFilesTab.tsx`
- Modify: `frontend/src/components/chat/WorkspaceGitBranchSheet.tsx`
- Modify: `frontend/src/components/chat/WorkspaceGitTab.tsx`
- Modify: `frontend/src/components/groups/GroupFormDialog.tsx`
- Modify: `frontend/src/pages/group/GroupChatPage.tsx`
- Modify: `frontend/src/pages/group/GroupManagePage.tsx`
- Modify: `frontend/src/pages/group/GroupMembersTab.tsx`
- Modify: `frontend/src/pages/group/GroupSchedulerSettingsSection.tsx`
- Modify: `frontend/src/pages/group/GroupSettingsTab.tsx`
- Modify: `frontend/src/components/chat/AgentActivityBubble.test.tsx`
- Modify: `frontend/src/components/chat/Composer.test.tsx`
- Modify: `frontend/src/components/chat/DispatchDag.test.tsx`
- Modify: `frontend/src/components/chat/MessageItem.test.tsx`
- Modify: `frontend/src/components/chat/MessageList.test.tsx`
- Modify: `frontend/src/components/chat/MarkdownMessage.test.tsx`
- Modify: `frontend/src/components/chat/StreamTimeline.test.tsx`
- Modify: `frontend/src/components/chat/TurnSummary.test.tsx`
- Modify: `frontend/src/components/chat/TurnTraceDrawer.test.tsx`
- Modify: `frontend/src/pages/group/GroupSchedulerSettingsSection.test.tsx`
- Modify: `frontend/src/stores/messageStore.ts`

**Interfaces:**
- Consumes: namespaces `chat`, `groups`, and `common` plus locale formatters.
- Produces: localized composer, message actions, stream timeline, tool activity, turn trace, workspace file/Git UI, group creation/management, and all accessibility labels.
- Preserves: Agent message content, tool arguments/results, file content, Git diffs, command output, and raw diagnostic detail.

- [ ] **Step 1: Add bilingual assertions to chat and group tests**

Extend `Composer.test.tsx`, `MessageList.test.tsx`, `StreamTimeline.test.tsx`, `TurnTraceDrawer.test.tsx`, and `GroupSchedulerSettingsSection.test.tsx`. Assert English and Chinese composer placeholder/cancel actions, older-message loading, stream state, trace metrics, scheduler labels, and error/empty states. Keep Agent-authored message assertions byte-for-byte unchanged in both locales.

- [ ] **Step 2: Run focused tests to verify the Chinese assertions fail**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/components/chat src/pages/group
```

Expected: the new Chinese assertions FAIL.

- [ ] **Step 3: Populate chat and group keys**

Organize `chat` keys under `composer`, `messages`, `stream`, `tools`, `trace`, `workspace`, and `errors`; organize `groups` under `create`, `manage`, `members`, `scheduler`, `settings`, and `errors`. Include explicit English/Chinese translations for every status currently rendered from enums. Use maps from enum value to translation key rather than translating unknown server text.

The resource contract includes these core keys and their Chinese counterparts:

```ts
chat: {
  composer: { placeholder: 'Message your agents…', send: 'Send message', stop: 'Stop generating' },
  messages: { loadOlder: 'Load older messages', empty: 'Start the conversation.', copied: 'Copied' },
  stream: { reasoning: 'Reasoning', running: 'Running', waiting: 'Waiting for you', error: 'Stream error: {{message}}' },
  trace: { title: 'Turn details', steps: 'Steps', hops: 'Hops', tokens: 'Tokens' },
  workspace: { show: 'Show workspace files', hide: 'Hide workspace files', resize: 'Resize workspace files column' },
},
groups: {
  header: { agent_one: '{{count}} agent', agent_other: '{{count}} agents', announcement: 'Announcement: {{text}}' },
  emptyAgents: 'No agents in this group yet — add one in group settings.',
  actions: { manage: 'Manage group', create: 'Create group', delete: 'Delete group' },
},
```

- [ ] **Step 4: Replace group/chat literals and locale-sensitive formatting**

Use locale format helpers in `AgentAvatar`, `DispatchDag`, `MessageItem`, `StreamTimeline`, and `TurnTraceDrawer`. Translate only UI framing around raw Agent/tool values. In `messageStore.ts`, keep stored warning payloads raw and let rendering components map known warning codes/text to translation keys so changing languages re-renders existing warnings.

- [ ] **Step 5: Run all frontend unit tests**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test
```

Expected: PASS.

- [ ] **Step 6: Commit translated chat and groups**

```powershell
git add frontend/src/i18n frontend/src/components/chat frontend/src/components/groups frontend/src/pages/group frontend/src/stores/messageStore.ts
git commit -m "feat(frontend): translate group chat experience"
```

---

### Task 7: Add localized API errors and run the complete translation audit

**Files:**
- Create: `frontend/src/lib/localizedApiError.ts`
- Create: `frontend/src/lib/localizedApiError.test.ts`
- Modify: `frontend/src/i18n/resources/en-US.ts`
- Modify: `frontend/src/i18n/resources/zh-CN.ts`
- Modify: callers that currently render `ApiError.message` directly

**Interfaces:**
- Produces: `localizedApiError(error: unknown, t: TFunction, fallbackKey: string): string`.
- Maps known `ApiError.code` values (`invalid_input`, `not_found`, `conflict`, `permission_denied`, `unauthorized`, `internal`) to localized generic copy while retaining raw details only in console/server logs.

- [ ] **Step 1: Write failing known/unknown error mapping tests**

Assert that a conflict error maps to localized conflict copy, an unknown error maps to the caller's fallback key, and no returned user-facing string contains an untranslated raw backend diagnostic.

- [ ] **Step 2: Run the error-helper test to verify failure**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/lib/localizedApiError.test.ts
```

Expected: FAIL because the helper does not exist.

- [ ] **Step 3: Implement and adopt the error mapper**

Implement an exhaustive code-to-key record and replace direct `String(error)` / `error.message` rendering in pages and forms. Log the original unknown error with `console.error` and return localized fallback copy.

- [ ] **Step 4: Audit remaining user-facing literals**

Run:

```powershell
rg -n --glob '*.tsx' ">[[:space:]]*[A-Za-z][^<{]*<|placeholder=\"[A-Za-z]|aria-label=\"[A-Za-z]|title=\"[A-Za-z]" frontend/src/components frontend/src/pages
rg -n "toLocale(DateString|TimeString|String)\(" frontend/src --glob '*.ts' --glob '*.tsx'
```

Expected: the first command returns only deliberate technical/sample content documented inline with `// i18n-ignore`; the second returns only implementations inside `frontend/src/lib/format.ts`.

- [ ] **Step 5: Run the complete verification suite**

Run:

```powershell
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend
pnpm --filter @ag-swarmer/frontend test
pnpm --filter @ag-swarmer/frontend lint
pnpm --filter @ag-swarmer/frontend type-check
pnpm --filter @ag-swarmer/frontend build
```

Expected: all commands PASS; the build emits production assets without missing-key warnings.

- [ ] **Step 6: Commit the completed bilingual surface**

```powershell
git add frontend/src/lib/localizedApiError.ts frontend/src/lib/localizedApiError.test.ts frontend/src/i18n frontend/src/components frontend/src/pages
git commit -m "feat(frontend): complete bilingual interface"
```
