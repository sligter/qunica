import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { z } from 'zod'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ApiError, fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { Token, UserRead } from '@/types/api'

interface AuthFormProps {
  mode: 'login' | 'register'
  onSuccess: () => void
}

export function AuthForm({ mode, onSuccess }: AuthFormProps) {
  const { t } = useTranslation('auth')
  const setToken = useAuthStore((s) => s.setToken)
  const setUser = useAuthStore((s) => s.setUser)
  const [submitError, setSubmitError] = useState<string | null>(null)

  const loginSchema = z.object({
    email: z.string().email(t('validation.validEmail')),
    password: z.string().min(8, t('validation.passwordLength')),
  })
  const schema =
    mode === 'login'
      ? loginSchema
      : loginSchema.extend({ name: z.string().min(1, t('validation.required')).max(100) })
  const form = useForm<{ email: string; password: string; name?: string }>({
    resolver: zodResolver(schema),
    defaultValues: { email: '', password: '', name: '' },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(null)
    try {
      if (mode === 'register') {
        await fetchJson<UserRead>('/auth/register', {
          method: 'POST',
          body: {
            email: values.email,
            password: values.password,
            name: values.name ?? '',
          },
        })
      }
      const token = await fetchJson<Token>('/auth/login', {
        method: 'POST',
        body: { email: values.email, password: values.password },
      })
      setToken(token.access_token)
      const me = await fetchJson<UserRead>('/auth/me', { token: token.access_token })
      setUser(me)
      onSuccess()
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        setSubmitError(t('errors.invalidCredentials'))
      } else if (err instanceof ApiError) {
        setSubmitError(err.message)
      } else {
        setSubmitError(t('errors.network'))
      }
    }
  })

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      {mode === 'register' && (
        <div className="space-y-1.5">
          <Label htmlFor="name">{t('fields.name')}</Label>
          <Input id="name" autoComplete="name" {...form.register('name')} />
          {form.formState.errors.name && (
            <p className="text-xs text-destructive">
              {form.formState.errors.name.message}
            </p>
          )}
        </div>
      )}
      <div className="space-y-1.5">
        <Label htmlFor="email">{t('fields.email')}</Label>
        <Input id="email" type="email" autoComplete="email" {...form.register('email')} />
        {form.formState.errors.email && (
          <p className="text-xs text-destructive">{form.formState.errors.email.message}</p>
        )}
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="password">{t('fields.password')}</Label>
        <Input
          id="password"
          type="password"
          autoComplete={mode === 'login' ? 'current-password' : 'new-password'}
          {...form.register('password')}
        />
        {form.formState.errors.password && (
          <p className="text-xs text-destructive">
            {form.formState.errors.password.message}
          </p>
        )}
      </div>
      {submitError && (
        <p className="text-sm text-destructive" role="alert">
          {submitError}
        </p>
      )}
      <Button type="submit" disabled={form.formState.isSubmitting} className="w-full">
        {t(mode === 'login' ? 'login.submit' : 'register.submit')}
      </Button>
    </form>
  )
}
