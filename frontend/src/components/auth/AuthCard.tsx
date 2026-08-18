import type { CSSProperties, ReactNode } from 'react'
import { MessagesSquare, Plug, Workflow } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { BrandMark } from '@/components/brand/BrandMark'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'

interface AuthCardProps {
  /** Card heading — the page's own title. */
  title: string
  /** One line under the heading saying what submitting the form does. */
  subtitle: string
  /** The form plus its switch-to-the-other-mode line. */
  children: ReactNode
}

/**
 * Read top to bottom, these answer "what is this?" before the form asks for a
 * password — a visitor who arrived from a release page has no other briefing.
 */
const HIGHLIGHTS = [
  { key: 'group', Icon: MessagesSquare },
  { key: 'scheduler', Icon: Workflow },
  { key: 'harness', Icon: Plug },
] as const

/** Entrance beats in ms: the mark lands, then the pitch, then the form. */
const rise = (delay: number): CSSProperties => ({ animationDelay: `${delay}ms` })

/**
 * The signed-out surface: the product introduces itself on the left, the form
 * waits on the right. Login and register share it so the two screens can't
 * drift apart.
 */
export function AuthCard({ title, subtitle, children }: AuthCardProps) {
  const { t } = useTranslation('auth')

  return (
    <div className="relative h-full overflow-y-auto bg-background">
      {/* Two warm fields drifting behind everything — depth without a texture
          file, and cheap: they animate transform only. */}
      <div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="animate-auth-aurora absolute -left-32 -top-32 h-[26rem] w-[26rem] rounded-full bg-primary/10 blur-3xl" />
        <div
          className="animate-auth-aurora absolute -bottom-40 -right-24 h-[30rem] w-[30rem] rounded-full bg-avatar-2/10 blur-3xl"
          style={{ animationDelay: '-9s' }}
        />
      </div>

      <div className="relative mx-auto flex min-h-full w-full max-w-5xl flex-col items-center justify-center gap-10 px-6 py-12 lg:flex-row lg:gap-16">
        <section className="flex w-full max-w-md flex-col items-center text-center lg:flex-1 lg:items-start lg:text-left">
          <div className="relative">
            <div aria-hidden className="absolute -inset-5 rounded-full bg-primary/15 blur-2xl" />
            <BrandMark animated className="relative h-20 w-20" />
          </div>
          <h1
            className="animate-auth-rise mt-6 font-serif text-4xl font-semibold tracking-tight"
            style={rise(140)}
          >
            AG Swarmer
          </h1>
          <p
            className="animate-auth-rise mt-2 text-base font-medium text-primary"
            style={rise(220)}
          >
            {t('brand.tagline')}
          </p>
          <p
            className="animate-auth-rise mt-3 max-w-sm text-sm leading-relaxed text-muted-foreground"
            style={rise(300)}
          >
            {t('brand.intro')}
          </p>
          {/* Below lg the window is short enough that the pitch would push the
              form off screen, so only the one-liner above survives. */}
          <ul className="mt-8 hidden w-full space-y-4 lg:block">
            {HIGHLIGHTS.map(({ key, Icon }, index) => (
              <li
                key={key}
                className="animate-auth-rise flex items-start gap-3"
                style={rise(380 + index * 80)}
              >
                <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
                  <Icon className="h-4 w-4" />
                </span>
                <div className="min-w-0">
                  <p className="text-sm font-medium">{t(`brand.highlights.${key}.title`)}</p>
                  <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">
                    {t(`brand.highlights.${key}.body`)}
                  </p>
                </div>
              </li>
            ))}
          </ul>
        </section>

        <div className="animate-auth-rise w-full max-w-sm" style={rise(260)}>
          <Card className="w-full shadow-lg">
            <CardHeader>
              <CardTitle>{title}</CardTitle>
              <CardDescription>{subtitle}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">{children}</CardContent>
          </Card>
        </div>
      </div>
    </div>
  )
}
