import type { CSSProperties, ReactNode } from 'react'
import { FolderKanban, MessagesSquare, Route, ShieldCheck } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { BrandMark } from '@/components/brand/BrandMark'

interface AuthCardProps {
  title: string
  subtitle: string
  children: ReactNode
}

const HIGHLIGHTS = [
  { key: 'context', Icon: MessagesSquare },
  { key: 'routing', Icon: Route },
  { key: 'local', Icon: ShieldCheck },
] as const

const rise = (delay: number): CSSProperties => ({ animationDelay: `${delay}ms` })

/** Login and registration share one product introduction and one form shell. */
export function AuthCard({ title, subtitle, children }: AuthCardProps) {
  const { t } = useTranslation('auth')

  return (
    <main className="relative h-full overflow-x-hidden overflow-y-auto bg-[#1a1816]">
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 opacity-35 [background-image:linear-gradient(rgba(255,255,255,.035)_1px,transparent_1px),linear-gradient(90deg,rgba(255,255,255,.035)_1px,transparent_1px)] [background-size:44px_44px]"
      />

      <div className="relative grid min-h-full min-w-0 grid-cols-[minmax(0,1fr)] lg:grid-cols-[minmax(0,1.12fr)_minmax(25rem,.88fr)]">
        <section className="relative flex min-h-[17rem] min-w-0 flex-col overflow-hidden px-6 py-7 text-[#fffaf2] sm:px-10 sm:py-9 lg:min-h-full lg:px-12 lg:py-10 xl:px-16">
          <div aria-hidden className="absolute -left-44 top-1/4 h-[32rem] w-[32rem] rounded-full bg-[#c85a38]/14 blur-3xl" />
          <div aria-hidden className="absolute -right-36 -top-44 h-[30rem] w-[30rem] rounded-full bg-[#d8a94e]/10 blur-3xl" />

          <header className="relative flex items-center justify-between">
            <div className="flex items-center gap-3">
              <BrandMark animated className="h-10 w-10" />
              <span className="font-serif text-xl font-semibold tracking-tight">Qunica</span>
            </div>
            <span className="rounded-full border border-white/10 bg-white/[0.045] px-3 py-1.5 text-[11px] font-medium tracking-wide text-[#fffaf2]/60">
              {t('brand.localBadge')}
            </span>
          </header>

          <div className="relative my-auto max-w-2xl py-8 lg:py-14">
            <p className="animate-auth-rise text-xs font-semibold uppercase tracking-[0.22em] text-[#e4a757]" style={rise(80)}>
              {t('brand.eyebrow')}
            </p>
            <h1 className="animate-auth-rise mt-4 max-w-xl font-serif text-[2rem] font-semibold leading-[1.08] tracking-[-0.035em] sm:text-5xl sm:leading-[1.04] xl:text-6xl" style={rise(150)}>
              {t('brand.headline')}
            </h1>
            <p className="animate-auth-rise mt-5 max-w-xl text-sm leading-7 text-[#fffaf2]/62 sm:text-base" style={rise(220)}>
              {t('brand.intro')}
            </p>

            <RoomPreview />
          </div>

          <ul className="relative hidden grid-cols-3 gap-5 border-t border-white/10 pt-6 lg:grid">
            {HIGHLIGHTS.map(({ key, Icon }, index) => (
              <li key={key} className="animate-auth-rise" style={rise(440 + index * 70)}>
                <Icon className="h-4 w-4 text-[#e4a757]" aria-hidden />
                <p className="mt-3 text-sm font-medium">{t(`brand.highlights.${key}.title`)}</p>
                <p className="mt-1.5 text-xs leading-5 text-[#fffaf2]/45">
                  {t(`brand.highlights.${key}.body`)}
                </p>
              </li>
            ))}
          </ul>
        </section>

        <section className="relative flex min-h-[34rem] min-w-0 items-center bg-background px-6 py-10 sm:px-10 lg:rounded-l-[2rem] lg:px-12 lg:shadow-[-24px_0_80px_rgba(0,0,0,.18)] xl:px-16">
          <div className="animate-auth-rise mx-auto w-full max-w-md" style={rise(180)}>
            <p className="text-xs font-semibold uppercase tracking-[0.18em] text-primary">
              {t('form.eyebrow')}
            </p>
            <h2 className="mt-3 font-serif text-3xl font-semibold tracking-[-0.025em]">{title}</h2>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">{subtitle}</p>

            <div className="mt-8">{children}</div>

            <div className="mt-8 flex items-start gap-3 border-t border-border/70 pt-5 text-xs leading-5 text-muted-foreground">
              <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0 text-primary" aria-hidden />
              <p>{t('form.localAccount')}</p>
            </div>
          </div>
        </section>
      </div>
    </main>
  )
}

function RoomPreview() {
  const { t } = useTranslation('auth')

  return (
    <div className="animate-auth-rise mt-8 hidden max-w-xl overflow-hidden rounded-2xl border border-white/10 bg-white/[0.045] shadow-2xl shadow-black/15 backdrop-blur-sm sm:block" style={rise(300)}>
      <div className="flex items-center justify-between border-b border-white/10 px-4 py-3">
        <div className="flex items-center gap-2 text-xs font-medium text-[#fffaf2]/75">
          <FolderKanban className="h-4 w-4 text-[#e4a757]" aria-hidden />
          {t('brand.preview.room')}
        </div>
        <span className="flex items-center gap-2 text-[11px] text-[#fffaf2]/45">
          <span className="h-1.5 w-1.5 rounded-full bg-emerald-400" />
          {t('brand.preview.status')}
        </span>
      </div>
      <div className="grid gap-4 p-4 sm:grid-cols-[1fr_auto] sm:items-end">
        <div className="rounded-xl bg-black/15 p-4">
          <p className="text-[11px] font-medium text-[#e4a757]">{t('brand.preview.you')}</p>
          <p className="mt-1.5 text-sm leading-6 text-[#fffaf2]/78">{t('brand.preview.request')}</p>
          <div className="mt-3 flex flex-wrap gap-2">
            <span className="rounded-md bg-white/[0.055] px-2 py-1 font-mono text-[10px] text-[#fffaf2]/48">README.md</span>
            <span className="rounded-md bg-white/[0.055] px-2 py-1 font-mono text-[10px] text-[#fffaf2]/48">release.yml</span>
          </div>
        </div>
        <div className="flex gap-2 sm:flex-col">
          {(['research', 'build', 'review'] as const).map((member, index) => (
            <span key={member} className="flex items-center gap-2 rounded-lg border border-white/8 bg-white/[0.035] px-2.5 py-2 text-[10px] text-[#fffaf2]/58">
              <span className="flex h-5 w-5 items-center justify-center rounded-md bg-[var(--member-color)] text-[9px] font-semibold text-white" style={{ '--member-color': `var(--color-avatar-${index + 1})` } as CSSProperties}>
                {index + 1}
              </span>
              {t(`brand.preview.${member}`)}
            </span>
          ))}
        </div>
      </div>
    </div>
  )
}
