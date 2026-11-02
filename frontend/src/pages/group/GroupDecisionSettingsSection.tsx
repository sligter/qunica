import { useTranslation } from 'react-i18next'

import { SettingsRow, SettingsSection } from '@/components/ui/settings-row'
import { Switch } from '@/components/ui/switch'
import { useUpdateGroup } from '@/hooks/useGroups'
import { ApiError } from '@/lib/api-v2/client'
import type { DecisionScenarioKey, GroupRead } from '@/types/api'
import { useEffect, useState } from 'react'

const SCENARIO_KEYS: DecisionScenarioKey[] = [
  'moderator_selection',
  'automatic_finish',
  'proactive_prefilter',
  'shell_risk',
  'skill_suggestion',
  'note_validation',
  'reply_outcome',
]

// Note: every router-mode thread has one fixed responder, so these scenarios,
// which pick among or prune several candidates, never run there.
const ROUTER_UNUSED: ReadonlySet<DecisionScenarioKey> = new Set([
  'moderator_selection',
  'automatic_finish',
  'proactive_prefilter',
])

interface GroupDecisionSettingsSectionProps {
  group: GroupRead
  /** Whether the account has a decision endpoint and key saved. */
  endpointConfigured?: boolean
}

/**
 * Per-group switches for the account's decision model. The endpoint itself
 * lives under Settings → Decision model; this section only decides whether
 * this group uses it and for which scenarios. Every switch saves instantly,
 * like the other toggles on the group settings tab.
 */
export function GroupDecisionSettingsSection({
  group,
  endpointConfigured,
}: GroupDecisionSettingsSectionProps) {
  const { t } = useTranslation(['groups', 'settings'])
  const update = useUpdateGroup(group.id)
  const [enabled, setEnabled] = useState(group.decision_enabled)
  const [scenarios, setScenarios] = useState(group.decision_scenarios)
  const [error, setError] = useState<string | null | undefined>(undefined)

  useEffect(() => {
    setEnabled(group.decision_enabled)
  }, [group.decision_enabled])
  useEffect(() => {
    setScenarios(group.decision_scenarios)
  }, [group.decision_scenarios])

  const visibleScenarios =
    group.scheduler_mode === 'router'
      ? SCENARIO_KEYS.filter((key) => !ROUTER_UNUSED.has(key))
      : SCENARIO_KEYS

  const errorMessage = (err: unknown): string | null =>
    err instanceof ApiError ? err.message : null

  const onEnabledChange = async (next: boolean) => {
    const previous = enabled
    setEnabled(next)
    setError(undefined)
    try {
      await update.mutateAsync({ decision_enabled: next })
    } catch (err) {
      setEnabled(previous)
      setError(errorMessage(err))
    }
  }

  const onScenarioChange = async (key: DecisionScenarioKey, next: boolean) => {
    const previous = scenarios
    setScenarios({ ...scenarios, [key]: next })
    setError(undefined)
    try {
      await update.mutateAsync({ decision_scenarios: { [key]: next } })
    } catch (err) {
      setScenarios(previous)
      setError(errorMessage(err))
    }
  }

  return (
    <SettingsSection
      title={t('settings:decision.title')}
      description={t('settings:decision.scenarios.description')}
    >
      <SettingsRow
        label={t('settings:decision.scenarios.enabled')}
        description={
          endpointConfigured === false
            ? t('settings:decision.scenarios.endpointMissing')
            : t('settings:decision.scenarios.enabledDescription')
        }
      >
        <Switch
          checked={enabled}
          onCheckedChange={(next) => void onEnabledChange(next)}
          disabled={update.isPending}
          aria-label={t('settings:decision.scenarios.enabled')}
        />
      </SettingsRow>
      {visibleScenarios.map((key) => (
        <SettingsRow
          key={key}
          label={t(`settings:decision.scenarios.${key}`)}
          description={t(`settings:decision.scenarios.${key}Description`)}
        >
          <Switch
            checked={scenarios[key]}
            onCheckedChange={(next) => void onScenarioChange(key, next)}
            disabled={update.isPending || !enabled}
            aria-label={t(`settings:decision.scenarios.${key}`)}
          />
        </SettingsRow>
      ))}
      {error !== undefined ? (
        <p className="py-2 text-sm text-destructive" role="alert">
          {error ? t('groups:errors.updateDetail', { message: error }) : t('groups:errors.update')}
        </p>
      ) : null}
    </SettingsSection>
  )
}
