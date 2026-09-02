import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, Navigate, useNavigate } from 'react-router-dom'

import { AuthCard } from '@/components/auth/AuthCard'
import { AuthForm } from '@/components/auth/AuthForm'
import { useAuthConfig } from '@/hooks/useAuthConfig'

export function RegisterPage() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()
  const authConfig = useAuthConfig()
  const title = t('register.title')

  useEffect(() => {
    document.title = title
  }, [title])

  if (authConfig.data?.registration_enabled === false) {
    return <Navigate to="/login" replace />
  }

  return (
    <AuthCard title={title} subtitle={t('register.subtitle')}>
      <AuthForm mode="register" onSuccess={() => void navigate('/')} />
      <p className="mt-6 text-center text-sm text-muted-foreground">
        {t('register.switchPrompt')}{' '}
        <Link to="/login" className="font-semibold text-primary underline-offset-4 hover:underline">
          {t('register.switchAction')}
        </Link>
      </p>
    </AuthCard>
  )
}
