"""Opt-in real Chromium font regression; no external network or paid model.

QUNICA_DOCKER_TESTS=1 python3 scripts/test-computer-fonts.py
QUNICA_TEST_COMPUTER_IMAGE selects a candidate image; QUNICA_FONT_SCREENSHOT
optionally retains its PNG screenshot on the host.
"""
import json
import os
from pathlib import Path
import subprocess
import time
import unittest
import uuid

# document.fonts.check() also succeeds with tofu/missing-glyph fallback. Query
# the actual platform fonts used for each text node instead, including glyphs.
FONT_CHECK = r'''
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { chromium } from '/usr/local/lib/node_modules/@playwright/mcp/node_modules/playwright-core/index.mjs'
assert.match(execFileSync('fc-list', [':lang=zh', 'family'], { encoding: 'utf8' }), /Noto.*CJK/)
const browser = await chromium.launch({
  executablePath: '/usr/bin/chromium', headless: false,
  args: ['--no-sandbox', '--disable-dev-shm-usage'],
})
try {
  const page = await browser.newPage({ viewport: { width: 1000, height: 560 } })
  const samples = [
    { id: 'simplified', lang: 'zh-CN', family: 'sans-serif', text: '中文截图字体显示正常', font: /Noto Sans CJK/ },
    { id: 'traditional', lang: 'zh-TW', family: 'serif', text: '繁體中文瀏覽器截圖測試', font: /Noto Serif CJK/ },
    { id: 'monospace', lang: 'zh-CN', family: 'monospace', text: '工作区文件交付测试', font: /Noto.*CJK/ },
    { id: 'emoji', lang: 'en', family: 'sans-serif', text: '🚀🧪🦄', font: /Noto Color Emoji/ },
  ]
  await page.setContent('<!doctype html><meta charset="utf-8"><style>body{padding:24px;background:#fff;color:#18212b}h1{font:24px sans-serif}p{font-size:32px;margin:24px 0}</style><h1>Qunica computer font regression</h1>' + samples.map(s => `<p id="${s.id}" lang="${s.lang}" style="font-family:${s.family}">${s.text}</p>`).join(''))
  await page.evaluate(() => document.fonts.ready)
  const client = await page.context().newCDPSession(page)
  await client.send('DOM.enable')
  await client.send('CSS.enable')
  const { root } = await client.send('DOM.getDocument')
  const results = []
  for (const sample of samples) {
    const { nodeId } = await client.send('DOM.querySelector', { nodeId: root.nodeId, selector: '#' + sample.id })
    const { fonts } = await client.send('CSS.getPlatformFontsForNode', { nodeId })
    assert.ok(fonts.some(f => sample.font.test(f.familyName) && f.glyphCount > 0), `${sample.id}: missing expected font; actual=${JSON.stringify(fonts)}`)
    assert.ok(fonts.every(f => sample.font.test(f.familyName) && !f.isCustomFont), `${sample.id}: unexpected fallback; actual=${JSON.stringify(fonts)}`)
    assert.equal(fonts.reduce((count, font) => count + font.glyphCount, 0), [...sample.text].length, `${sample.id}: incomplete glyph coverage`)
    results.push({ sample: sample.id, fonts })
  }
  await page.screenshot({ path: '/tmp/qunica-font-regression.png' })
  console.log(JSON.stringify(results))
} finally { await browser.close() }
'''


@unittest.skipUnless(os.environ.get('QUNICA_DOCKER_TESTS') == '1', 'opt-in real Docker font test')
class ComputerFontTests(unittest.TestCase):
    def test_chromium_renders_chinese_and_emoji_with_real_system_fonts(self):
        name = 'qunica-font-test-' + uuid.uuid4().hex[:12]
        image = os.environ.get('QUNICA_TEST_COMPUTER_IMAGE', 'qunica-computer:local')
        try:
            subprocess.run([
                'docker', 'run', '-d', '--name', name, '--read-only',
                '--network', 'none', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges:true',
                '--user', '10001:10001', '--init', '--env', 'DISPLAY=:99', '--workdir', '/tmp',
                '--cpus', '2', '--memory', '4g', '--memory-swap', '4g', '--pids-limit', '512',
                '--tmpfs', '/tmp:rw,nosuid,nodev,size=134217728,mode=1777',
                '--tmpfs', '/home/agent:rw,nosuid,nodev,uid=10001,gid=10001', image,
            ], check=True, capture_output=True)
            for _ in range(100):
                ready = subprocess.run(['docker', 'exec', name, 'python3', '-c',
                                        'import socket;socket.create_connection(("127.0.0.1",8765),timeout=.1).close()'],
                                       capture_output=True)
                if ready.returncode == 0:
                    break
                time.sleep(.1)
            self.assertEqual(ready.returncode, 0, ready.stderr.decode())
            result = subprocess.run(['docker', 'exec', '-i', name, 'node', '--input-type=module'],
                                    input=FONT_CHECK, text=True, capture_output=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            results = json.loads(result.stdout)
            self.assertEqual(len(results), 4)
            print(json.dumps(results, ensure_ascii=False))
            screenshot = os.environ.get('QUNICA_FONT_SCREENSHOT')
            if screenshot:
                path = Path(screenshot).resolve()
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(subprocess.check_output([
                    'docker', 'exec', name, 'python3', '-c',
                    'import sys;from pathlib import Path;sys.stdout.buffer.write(Path("/tmp/qunica-font-regression.png").read_bytes())',
                ]))
        finally:
            subprocess.run(['docker', 'rm', '-f', name], capture_output=True)


if __name__ == '__main__':
    unittest.main()
