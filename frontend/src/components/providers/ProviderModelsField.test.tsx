import { useState } from 'react'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it } from 'vitest'

import {
  ProviderModelsField,
  type ProviderModelDraft,
} from '@/components/providers/ProviderModelsField'
import i18n from '@/i18n'

afterEach(async () => {
  cleanup()
  await i18n.changeLanguage('en-US')
})

function Harness() {
  const [models, setModels] = useState<ProviderModelDraft[]>([
    { id: 'model-a', context_window_tokens: 32000, context_output_reserve_percent: 20 },
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
    await user.type(modelInputs[1], 'model-b')
    await user.click(screen.getAllByRole('radio', { name: 'Default' })[1])

    expect(modelInputs[1]).toHaveValue('model-b')
    expect(screen.getByRole('status')).toHaveTextContent('model-b')
  })
})
