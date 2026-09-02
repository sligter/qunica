import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { AlertCircle, ArrowRight, Eye, EyeOff, Loader2, LockKeyhole, Mail, UserRound } from 'lucide-react'
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
  const [showPassword, setShowPassword] = useState(false)
  const isLogin = mode === 'login'

  const loginSchema = z.object({
    email: z.string().trim().email(t('validation.validEmail')),
    password: z.string().min(8, t('validation.passwordLength')),
  })
  const schema = isLogin
    ? loginSchema
    : loginSchema.extend({
        name: z.string().trim().min(1, t('validation.required')).max(100, t('validation.nameLength')),
      })
  const form = useForm<{ email: string; password: string; name?: string }>({
    resolver: zodResolver(schema),
    defaultValues: { email: '', password: '', name: '' },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    setSubmitError(null)
    try {
      if (!isLogin) {
        await fetchJson<UserRead>('/auth/register', {
          method: 'POST',
          body: { email: values.email, password: values.password, name: values.name ?? '' },
        })
      }
      const token = await fetchJson<Token>('/auth/login', {
        method: 'POST',
        body: { email: values.email, password: values.password },
      })
      setToken(token.access_token)
      setUser(await fetchJson<UserRead>('/auth/me', { token: token.access_token }))
      onSuccess()
    } catch (err) {
      if (err instanceof ApiError) {
        if (isLogin && (err.status === 401 || err.status === 403)) {
          setSubmitError(t('errors.invalidCredentials'))
        } else if (!isLogin && err.status === 409) {
          setSubmitError(t('errors.userExists'))
        } else if (!isLogin && err.code === 'registration_disabled') {
          setSubmitError(t('errors.registrationDisabled'))
        } else {
          setSubmitError(t('errors.generic'))
        }
      } else {
        setSubmitError(t('errors.network'))
      }
    }
  })

  return (
    <form onSubmit={onSubmit} noValidate className="space-y-5">
      {!isLogin && (
        <div className="space-y-2">
          <Label htmlFor="name">{t('fields.name')}</Label>
          <div className="relative">
            <UserRound className="pointer-events-none absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
            <Input
              id="name"
              autoComplete="name"
              autoFocus
              aria-invalid={Boolean(form.formState.errors.name)}
              aria-describedby={form.formState.errors.name ? 'name-error' : undefined}
              className="h-11 pl-10"
              {...form.register('name')}
            />
          </div>
          {form.formState.errors.name && <p id="name-error" className="text-xs text-destructive">{form.formState.errors.name.message}</p>}
        </div>
      )}

      <div className="space-y-2">
        <Label htmlFor="email">{t('fields.email')}</Label>
        <div className="relative">
          <Mail className="pointer-events-none absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
          <Input
            id="email"
            type="email"
            autoComplete="email"
            autoCapitalize="none"
            spellCheck={false}
            autoFocus={isLogin}
            aria-invalid={Boolean(form.formState.errors.email)}
            aria-describedby={form.formState.errors.email ? 'email-error' : undefined}
            className="h-11 pl-10"
            {...form.register('email')}
          />
        </div>
        {form.formState.errors.email && <p id="email-error" className="text-xs text-destructive">{form.formState.errors.email.message}</p>}
      </div>

      <div className="space-y-2">
        <Label htmlFor="password">{t('fields.password')}</Label>
        <div className="relative">
          <LockKeyhole className="pointer-events-none absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
          <Input
            id="password"
            type={showPassword ? 'text' : 'password'}
            autoComplete={isLogin ? 'current-password' : 'new-password'}
            aria-invalid={Boolean(form.formState.errors.password)}
            aria-describedby={form.formState.errors.password ? 'password-error' : !isLogin ? 'password-hint' : undefined}
            className="h-11 px-10"
            {...form.register('password')}
          />
          <button
            type="button"
            aria-label={t(showPassword ? 'password.hide' : 'password.show')}
            aria-pressed={showPassword}
            onClick={() => setShowPassword((visible) => !visible)}
            className="absolute right-1.5 top-1/2 flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {showPassword ? <EyeOff className="h-4 w-4" aria-hidden /> : <Eye className="h-4 w-4" aria-hidden />}
          </button>
        </div>
        {form.formState.errors.password ? (
          <p id="password-error" className="text-xs text-destructive">{form.formState.errors.password.message}</p>
        ) : !isLogin ? (
          <p id="password-hint" className="text-xs text-muted-foreground">{t('password.hint')}</p>
        ) : null}
      </div>

      {submitError && (
        <div className="flex items-start gap-2.5 rounded-lg border border-destructive/20 bg-destructive/5 px-3.5 py-3 text-sm text-destructive" role="alert">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
          <span>{submitError}</span>
        </div>
      )}

      <Button type="submit" disabled={form.formState.isSubmitting} className="h-11 w-full">
        {form.formState.isSubmitting ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden /> : null}
        {t(isLogin ? 'login.submit' : 'register.submit')}
        {!form.formState.isSubmitting ? <ArrowRight className="h-4 w-4" aria-hidden /> : null}
      </Button>
    </form>
  )
}
