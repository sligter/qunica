"""Bounded file operations inside the selected computer; no host paths."""
import glob
import json
import os
from pathlib import Path
import re
import sys
import tempfile

ROOT = Path.cwd().resolve()
MAX_BYTES = 96 * 1024


def path(value):
    p = Path(value)
    if not value or p.is_absolute() or '..' in p.parts:
        raise ValueError('expected a workspace-relative path without traversal')
    p = (ROOT / p).resolve()
    if not p.is_relative_to(ROOT):
        raise ValueError('path escapes the computer workspace')
    return p


def read(p):
    with p.open('rb') as f:
        raw = f.read(MAX_BYTES + 1)
    if len(raw) > MAX_BYTES:
        raise ValueError('file exceeds the 96 KiB file-tool limit; use a bounded shell command')
    return raw.decode('utf-8')


def write(p, text):
    if len(text.encode('utf-8')) > MAX_BYTES:
        raise ValueError('file content exceeds limit')
    p.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(dir=p.parent)
    try:
        with os.fdopen(fd, 'w', encoding='utf-8', newline='') as f:
            f.write(text)
        os.replace(name, p)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def run(tool, a):
    if tool in ('Read', 'Write', 'Edit', 'DeleteFile'):
        p = path(a.get('path', a.get('file_path', '')))
        if tool == 'Read':
            offset = max(1, int(a.get('offset', a.get('start_line', 1))))
            limit = min(1000, max(1, int(a.get('limit', 200))))
            return '\n'.join(f'{n}: {line}' for n, line in enumerate(read(p).splitlines(), 1) if offset <= n < offset + limit)
        if tool == 'Write':
            write(p, a['content'])
            return 'written'
        if tool == 'DeleteFile':
            if not p.is_file():
                raise ValueError('expected a file')
            p.unlink()
            return 'deleted'
        text = read(p)
        edits = a.get('edits') or [{'oldText': a.get('oldText', a.get('old_string')), 'newText': a.get('newText', a.get('new_string'))}]
        for edit in edits:
            old, new = edit['oldText'], edit['newText']
            if not isinstance(old, str) or not old or not isinstance(new, str):
                raise ValueError('each edit requires nonempty oldText and string newText')
            count = text.count(old)
            if not count or (count > 1 and not a.get('replace_all', False)):
                raise ValueError('oldText must match exactly once unless replace_all is true')
            text = text.replace(old, new)
        write(p, text)
        return 'edited'
    pattern = a.get('pattern', '**/*') if tool == 'Glob' else a.get('path', '**/*')
    path(pattern)  # Reject absolute patterns and traversal before globbing.
    limit = min(200, max(1, int(a.get('limit', 100))))
    regex = re.compile(a['pattern']) if tool == 'Grep' else None
    result = []
    for checked, name in enumerate(glob.iglob(pattern, root_dir=ROOT, recursive=True)):
        if checked >= 4096 or len(result) >= limit:
            break
        p = path(name)
        if tool == 'Glob':
            result.append(name)
        elif p.is_file():
            try:
                lines = read(p).splitlines()
            except (UnicodeError, ValueError):
                continue
            for n, line in enumerate(lines, 1):
                if regex.search(line):
                    result.append(f'{name}:{n}: {line[:1000]}')
                    if len(result) >= limit:
                        break
    return '\n'.join(result)


try:
    request = json.loads(sys.argv[1])
    print(run(request['tool'], request['args'])[:MAX_BYTES])
except Exception as error:
    print(str(error)[:2000], file=sys.stderr)
    sys.exit(1)
