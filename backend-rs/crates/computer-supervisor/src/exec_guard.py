"""A confirmed Linux exec exit includes every descendant's exit.

Note: killpg misses new-session descendants. A subreaper adopts these orphans;
unknown guard termination is fenced by the caller, rather than releasing a job.
"""
import ctypes
import os
import signal
import subprocess
import sys
import time

stopping = False


def children():
    with open('/proc/self/task/' + str(os.getpid()) + '/children') as source:
        return [int(value) for value in source.read().split()]


def kill_children():
    for pid in children():
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def stop(_signal, _frame):
    global stopping
    stopping = True
    kill_children()


def reap():
    killed = False
    while True:
        try:
            while os.waitpid(-1, os.WNOHANG)[0]:
                pass
        except ChildProcessError:
            pass
        if not children():
            return killed
        killed = True
        kill_children()
        time.sleep(.01)


try:
    if ctypes.CDLL(None).prctl(36, 1, 0, 0, 0) != 0:  # PR_SET_CHILD_SUBREAPER
        raise RuntimeError('Subreaper unavailable')
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    child = subprocess.Popen(sys.argv[1:], stdin=sys.stdin.buffer)
    code = child.wait()
    background = reap()
    if background and not stopping and code == 0:
        print('Background descendants were stopped; computer commands must run in the foreground.', file=sys.stderr)
        code = 126
    sys.exit(0 if stopping else (code if code >= 0 else 128 - code))
except SystemExit:
    raise
except BaseException:
    try:
        kill_children()
        reap()
    finally:
        os._exit(253)
