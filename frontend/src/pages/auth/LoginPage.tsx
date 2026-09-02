import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate } from 'react-router-dom'

import { AuthCard } from '@/components/auth/AuthCard'
import { AuthForm } from '@/components/auth/AuthForm'
import { useAuthConfig } from '@/hooks/useAuthConfig'

export function LoginPage() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()
  const authConfig = useAuthConfig()
  const title = t('login.title')

  useEffect(() => {
    document.title = title
  }, [title])

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
