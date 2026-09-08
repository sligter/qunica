import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, Navigate, useNavigate } from 'react-router-dom'

import { AuthCard } from '@/components/auth/AuthCard'
import { AuthForm } from '@/components/auth/AuthForm'
import { useAuthConfig } from '@/hooks/useAuthConfig'
import { useAuthStore } from '@/stores/authStore'

export function LoginPage() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()
  const token = useAuthStore(s => s.token)
  const authConfig = useAuthConfig()
  const title = t('login.title')

  useEffect(() => {
    document.title = title
  }, [title])

  if (token) return <Navigate to="/" replace />

  return (
    <AuthCard title={title} subtitle={t('login.subtitle')}>
      <AuthForm mode="login" onSuccess={() => void navigate('/')} />
      {authConfig.data?.registration_enabled !== false && (
        <p className="mt-6 text-center text-sm text-muted-foreground">
          {t('login.switchPrompt')}{' '}
          <Link to="/register" className="font-semibold text-primary underline-offset-4 hover:underline">
            {t('login.switchAction')}
          </Link>
        </p>
      )}
    </AuthCard>
  )
}
