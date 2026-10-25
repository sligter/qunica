"""Desktop startup regression; QUNICA_DOCKER_TESTS=1 also checks real RFB pixels."""
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
import uuid

ROOT = Path(__file__).resolve().parent.parent
STUB = '''#!{python}
import os, pathlib, sys, time
name = pathlib.Path(sys.argv[0]).name
root = pathlib.Path(os.environ['STUB_ROOT'])
with (root/'events').open('a') as out: out.write(name+' '+' '.join(sys.argv[1:])+'\\n')
if name in ['Xvfb', 'openbox', 'x11vnc']:
    if os.environ.get('FAIL_READY') == 'exit-'+name: sys.exit(1)
    with (root/'pids').open('a') as out: out.write(str(os.getpid())+'\\n')
    time.sleep(60)
elif name in ['xdotool', 'xprop', 'python3']:
    failure = os.environ.get('FAIL_READY')
    if failure == name or (failure == 'vnc' and name == 'python3'): sys.exit(1)
    counter = root/(name+'.count')
    count = int(counter.read_text()) if counter.exists() else 0
    counter.write_text(str(count+1))
    if count == 0: sys.exit(1)
    if name == 'xprop': print('_NET_SUPPORTING_WM_CHECK(WINDOW): window id # 0x100')
elif name == 'sleep': time.sleep(0.001)
'''


class StartupTests(unittest.TestCase):
    def run_startup(self, failure=''):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bin_dir = root/'bin'
            bin_dir.mkdir()
            for name in ['Xvfb', 'openbox', 'x11vnc', 'xmessage', 'xsetroot', 'xset',
                         'xdotool', 'xprop', 'python3', 'node', 'sleep']:
                path = bin_dir/name
                path.write_text(STUB.format(python=sys.executable))
                path.chmod(0o755)
            env = {**os.environ, 'HOME': str(root/'home'), 'TMPDIR': str(root), 'STUB_ROOT': str(root),
                   'FAIL_READY': failure, 'PATH': str(bin_dir)+os.pathsep+os.environ['PATH']}
            try:
                result = subprocess.run(['sh', str(ROOT/'docker/computer-start.sh')],
                                        env=env, capture_output=True, text=True, timeout=30)
                events = (root/'events').read_text().splitlines()
                return result, events
            finally:
                if (root/'pids').exists():
                    for pid in (root/'pids').read_text().splitlines():
                        try:
                            os.kill(int(pid), signal.SIGTERM)
                        except ProcessLookupError:
                            pass

    def test_services_wait_for_readiness_before_broker(self):
        result, events = self.run_startup()
        self.assertEqual(result.returncode, 0, result.stderr)
        def index(prefix):
            return next(i for i, event in enumerate(events) if event.startswith(prefix))
        self.assertLess(index('xdotool '), index('openbox '))
        self.assertGreater(index('xset s off'), index('xprop '))
        self.assertTrue(any(e.startswith('xset s noblank') for e in events))
        self.assertLess(index('xprop '), index('xsetroot '))
        self.assertLess(index('xmessage '), index('node '))
        probes = [e for e in events if e.startswith('python3 ')]
        self.assertGreaterEqual(len(probes), 3)
        self.assertTrue(any(e.endswith(' 5900') for e in probes))
        self.assertTrue(any(e.endswith(' 5901') for e in probes))
        self.assertIn('-viewonly', next(e for e in events if e.startswith('x11vnc ') and '5901' in e))
        self.assertGreater(index('node '), max(i for i, e in enumerate(events) if e.startswith('python3 ')))

    def test_x_failure_never_starts_window_manager_or_broker(self):
        result, events = self.run_startup('xdotool')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('X display did not become ready', result.stderr)
        self.assertFalse(any(e.startswith(('openbox ', 'node ')) for e in events))

    def test_wm_failure_never_starts_vnc_or_broker(self):
        result, events = self.run_startup('xprop')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Window manager did not become ready', result.stderr)
        self.assertFalse(any(e.startswith(('x11vnc ', 'node ')) for e in events))

    def test_exited_window_manager_never_starts_broker(self):
        result, events = self.run_startup('exit-openbox')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Window manager exited during startup', result.stderr)
        self.assertFalse(any(e.startswith(('x11vnc ', 'node ')) for e in events))

    def test_vnc_failure_never_starts_broker(self):
        result, events = self.run_startup('vnc')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('VNC 5900 did not become ready', result.stderr)
        self.assertFalse(any(e.startswith('node ') for e in events))


# Runs inside the test computer. A handshake alone cannot distinguish a working
# desktop from a connected black framebuffer; inspect actual raw RFB pixels.
PIXEL_CHECK = '''
import socket, struct, time
from collections import Counter

def exact(s, n):
    result=b''
    while len(result)<n:
        chunk=s.recv(n-len(result))
        assert chunk, 'RFB closed early'
        result+=chunk
    return result

for port in [5900,5901]:
    with socket.create_connection(('127.0.0.1',port),timeout=10) as s:
        assert exact(s,12)==b'RFB 003.008\\n'
        s.sendall(b'RFB 003.008\\n')
        types=exact(s,exact(s,1)[0]); assert 1 in types
        s.sendall(b'\\x01'); assert exact(s,4)==bytes(4)
        s.sendall(b'\\x01')
        init=exact(s,24)
        width,height=struct.unpack('>HH',init[:4]); assert (width,height)==(1280,800)
        exact(s,struct.unpack('>I',init[20:])[0])
        pixel_format=struct.pack('>BBBBHHHBBBxxx',32,24,0,1,255,255,255,16,8,0)
        s.sendall(bytes(4)+pixel_format)
        s.sendall(struct.pack('>BBHi',2,0,1,0))
        # Window mapping/decorations can finish just after broker readiness.
        # Observe bounded fresh frames instead of treating the first as final.
        for _ in range(30):
            s.sendall(struct.pack('>BBHHHH',3,0,240,220,800,360))
            update=exact(s,4); assert update[0]==0
            colors=Counter()
            for _ in range(struct.unpack('>H',update[2:])[0]):
                x,y,w,h,encoding=struct.unpack('>HHHHi',exact(s,12)); assert encoding==0
                pixels=exact(s,w*h*4)
                colors.update(pixels[i:i+3] for i in range(0,len(pixels),4))
            # Core X text may be monochrome: blue + black + white is a valid
            # ready notice. Count visible text, not antialiased color diversity.
            if colors.get(bytes([255,255,255]),0)>=100: break
            time.sleep(.1)
        assert colors.get(bytes([255,255,255]),0)>=100, f'port {port} missing ready notice: {colors}'
        assert bytes([0x47,0x34,0x24]) in colors, f'port {port} missing idle background'
        print(f'port {port}: {width}x{height}, {len(colors)} pixel colors, visible desktop')
'''


@unittest.skipUnless(os.environ.get('QUNICA_DOCKER_TESTS') == '1', 'opt-in real Docker desktop test')
class DockerDesktopTests(unittest.TestCase):
    def test_cold_start_has_window_manager_and_visible_rfb_pixels(self):
        for _ in range(3):
            name = 'qunica-desktop-test-'+uuid.uuid4().hex[:12]
            try:
                subprocess.run(['docker', 'run', '-d', '--name', name, '--read-only',
                                '--network', 'none', '--cap-drop', 'ALL',
                                '--security-opt', 'no-new-privileges:true', '--user', '10001:10001',
                                '--init', '--env', 'DISPLAY=:99', '--workdir', '/tmp',
                                '--cpus', '2', '--memory', '4g', '--memory-swap', '4g', '--pids-limit', '512',
                                '--tmpfs', '/tmp:rw,nosuid,nodev,size=134217728,mode=1777',
                                '--tmpfs', '/home/agent:rw,nosuid,nodev,uid=10001,gid=10001',
                                'qunica-computer:local'], check=True, capture_output=True)
                for _ in range(100):
                    ready = subprocess.run(['docker', 'exec', name, 'python3', '-c',
                                            'import socket;socket.create_connection(("127.0.0.1",8765),timeout=.1).close()'],
                                           capture_output=True)
                    if ready.returncode == 0:
                        break
                    time.sleep(.1)
                if ready.returncode != 0:
                    logs = subprocess.run(['docker', 'logs', name], capture_output=True, text=True)
                    self.fail(ready.stderr.decode() + logs.stdout + logs.stderr)
                subprocess.run(['docker', 'exec', name, 'sh', '-c',
                                'xprop -root _NET_SUPPORTING_WM_CHECK | grep "window id #"; '
                                'xset q | grep -E "timeout: +0 +cycle:"'], check=True)
                subprocess.run(['docker', 'exec', '-i', name, 'python3', '-c', PIXEL_CHECK], check=True)
            finally:
                subprocess.run(['docker', 'rm', '-f', name], capture_output=True)


if __name__ == '__main__':
    unittest.main()
