import type { enUS } from './en-US'

type TranslationShape<T> = { [K in keyof T]: T[K] extends string ? string : TranslationShape<T[K]> }

export const zhCN: TranslationShape<typeof enUS> = {
  common: {
    actions: { save: '保存', cancel: '取消', clear: '清除', delete: '删除', retry: '重试' },
    state: { loading: '加载中…', unavailable: '不可用' },
  },
  auth: {}, navigation: {}, chat: {}, agents: {}, groups: {}, providers: {}, skills: {}, workspaces: {}, settings: {},
}
