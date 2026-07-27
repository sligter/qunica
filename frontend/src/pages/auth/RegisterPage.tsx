import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate } from 'react-router-dom'

import { AuthCard } from '@/components/auth/AuthCard'
import { AuthForm } from '@/components/auth/AuthForm'

export function RegisterPage() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()
  const title = t('register.title')

  useEffect(() => {
    document.title = title
  }, [title])

  return (
    <AuthCard title={title}>
      <AuthForm mode="register" onSuccess={() => void navigate('/')} />
      <p className="text-sm text-muted-foreground">
        {t('register.switchPrompt')}{' '}
        <Link to="/login" className="font-medium text-foreground hover:underline">
          {t('register.switchAction')}
        </Link>
      </p>
    </AuthCard>
  )
}
