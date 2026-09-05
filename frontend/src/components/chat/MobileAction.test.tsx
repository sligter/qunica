import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'
import '@/i18n'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MobileActionContext } from './MobileAction'
import { HumanInputRequestForm } from './HumanInputRequestForm'

afterEach(cleanup)

it('moves the existing input to the mobile action slot and preserves the answer on failure', async () => {
  const slot = document.createElement('div')
  document.body.append(slot)
  const onSubmitResponse = vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValue(undefined)
  try {
    render(<QueryClientProvider client={new QueryClient()}><MobileActionContext.Provider value={slot}>
      <HumanInputRequestForm request={{ question: 'Choose a budget', input_type: 'text' }} onSubmitResponse={onSubmitResponse} />
    </MobileActionContext.Provider></QueryClientProvider>)
    expect(within(slot).getByText('Choose a budget')).toBeInTheDocument()
    const input = within(slot).getByRole('textbox')
    fireEvent.change(input, { target: { value: '100' } })
    fireEvent.submit(input.closest('form')!)
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('offline'))
    expect(input).toHaveValue('100')
    fireEvent.submit(input.closest('form')!)
    await waitFor(() => expect(slot.querySelector('form')).toBeNull())
    expect(onSubmitResponse).toHaveBeenCalledTimes(2)
  } finally { slot.remove() }
})
