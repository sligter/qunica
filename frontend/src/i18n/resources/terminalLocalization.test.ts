import { describe, expect, it } from 'vitest'

import { enUS } from './en-US'
import { zhCN } from './zh-CN'

describe('terminal localization resources', () => {
  it('keeps the English and Chinese terminal key sets identical', () => {
    expect(Object.keys(zhCN.chat.terminal).sort()).toEqual(
      Object.keys(enUS.chat.terminal).sort(),
    )
  })

  it('uses explicit natural Chinese full-shell copy', () => {
    expect(zhCN.chat.terminal.show).toBe('显示终端')
    expect(zhCN.chat.terminal.hide).toBe('隐藏终端')
    expect(zhCN.chat.terminal.fullAccessBody).toContain('Shell 可以离开工作区')
    expect(zhCN.chat.terminal.desktopRequired).toContain('桌面应用')
  })
})
