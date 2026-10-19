"""Resolve a task branch inside this computer, never a host worktree path."""
from pathlib import Path
import subprocess
import sys


def git(*args):
    return subprocess.run(['git', *args], check=True, capture_output=True, text=True).stdout.strip()


try:
    group, thread, branch = sys.argv[1:]
    if any(not part or len(part) > 64 or any(not (c.isascii() and (c.isalnum() or c == '-')) for c in part) for part in (group, thread)):
        raise ValueError('Invalid task identity')
    root = Path.cwd().resolve()
    if Path(git('rev-parse', '--show-toplevel')).resolve() != root:
        raise ValueError('Clone the project into the computer workspace before running a branch task')
    git('check-ref-format', '--branch', branch)
    target = (root / '.worktrees' / group / thread).resolve()
    if not target.is_relative_to(root):
        raise ValueError('Task worktree escapes the computer workspace')
    if not target.exists():
        target.parent.mkdir(parents=True, exist_ok=True)
        # Note: keeping worktrees out of the shared checkout's status prevents
        # unrelated task files entering a later commit. No tracked file changes.
        exclude = Path(git('rev-parse', '--git-path', 'info/exclude'))
        exclude.parent.mkdir(parents=True, exist_ok=True)
        existing = exclude.read_text() if exclude.exists() else ''
        if '/.worktrees/' not in existing.splitlines():
            with exclude.open('a') as output:
                output.write('\n/.worktrees/\n')
        exists = subprocess.run(['git', 'show-ref', '--verify', '--quiet', 'refs/heads/' + branch]).returncode
        if exists == 0:
            git('worktree', 'add', '--', str(target), branch)
        elif exists == 1:
            git('worktree', 'add', '-b', branch, '--', str(target), 'HEAD')
        else:
            raise ValueError('Computer repository refs are unavailable')
    if Path(git('-C', str(target), 'rev-parse', '--show-toplevel')).resolve() != target or git('-C', str(target), 'symbolic-ref', '--short', 'HEAD') != branch:
        raise ValueError('Computer task worktree no longer matches its branch')
    print(target.relative_to(root))
except ValueError as error:
    print(str(error), file=sys.stderr)
    sys.exit(1)
except Exception:
    print('Computer task worktree is unavailable; inspect the repository and branch on the computer.', file=sys.stderr)
    sys.exit(1)
