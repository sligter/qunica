import { useState } from 'react'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  ProviderModelsField,
  type ProviderModelDraft,
} from '@/components/providers/ProviderModelsField'
import i18n from '@/i18n'
import type { ProviderModelTestResult } from '@/types/api'

afterEach(async () => {
  cleanup()
  await i18n.changeLanguage('en-US')
})

function Harness({
  onTestModel = async () => ({ ok: true, latency_ms: 1, error: null }),
}: {
  onTestModel?: (modelId: string) => Promise<ProviderModelTestResult>
}) {
  const [models, setModels] = useState<ProviderModelDraft[]>([
    {
      id: 'model-a',
      context_window_tokens: 32000,
      context_output_reserve_percent: 20,
      reasoning_passback: false,
    },
  ])
  const [defaultModel, setDefaultModel] = useState('model-a')
  return (
    <>
      <ProviderModelsField
        models={models}
        defaultModel={defaultModel}
        catalog={[
          { id: 'model-a', name: 'Model A' },
          { id: 'model-b', name: 'Model B' },
        ]}
        showReasoningPassback
        onTestModel={onTestModel}
        onChange={setModels}
        onDefaultChange={setDefaultModel}
      />
      <output>{defaultModel}</output>
    </>
  )
}

describe('ProviderModelsField', () => {
  it('adds a discovered model and lets it become the default', async () => {
    const user = userEvent.setup()
    render(<Harness />)

    await user.click(screen.getByRole('button', { name: 'Add model' }))
    const modelInputs = screen.getAllByRole('combobox', { name: 'Model ID' })
    await user.click(modelInputs[1])
    await user.click(screen.getByRole('option', { name: /Model B/ }))
    await user.click(screen.getAllByRole('radio', { name: 'Default' })[1])

    expect(modelInputs[1]).toHaveValue('model-b')
    expect(screen.getByRole('status')).toHaveTextContent('model-b')
  })

  it('tests and configures each model independently', async () => {
    const user = userEvent.setup()
    const onTestModel = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, latency_ms: 42, error: null })
      .mockResolvedValueOnce({ ok: false, latency_ms: null, error: 'HTTP 401' })
    render(<Harness onTestModel={onTestModel} />)

    await user.click(screen.getByRole('button', { name: 'Test model model-a' }))
    expect(onTestModel).toHaveBeenCalledWith('model-a')
    expect(await screen.findByText(/responded successfully \(42 ms\)/i)).toBeVisible()

    await user.click(screen.getByRole('button', { name: 'Test model model-a' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(/failed.*HTTP 401/i)

    await user.click(
      screen.getByRole('switch', { name: 'Reasoning passback for model model-a' }),
    )
    expect(
      screen.getByRole('switch', { name: 'Reasoning passback for model model-a' }),
    ).toBeChecked()
  })
})
