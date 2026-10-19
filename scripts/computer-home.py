"""Operator-only consistent computer backup/restore. The app never runs Docker.

Both commands confirm a stopped container through the Supervisor first. Restore
requires the computer UUID and refuses archives from a different owner/binding.
"""
import argparse
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import urllib.request
import uuid
import zipfile

LIMIT = 64 * 1024 * 1024


def local_path(value):
    path = Path(value)
    if path.is_absolute() or '..' in path.parts:
        raise ValueError('Use a workspace-relative path')
    path = path.resolve()
    if not path.is_relative_to(Path.cwd().resolve()):
        raise ValueError('Path escapes the workspace')
    return path


def validate_archive(data):
    if len(data) > LIMIT:
        raise ValueError('Archive exceeds 64 MiB')
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        total = 0
        for item in archive:
            parts = Path(item.name.replace('\\', '/')).parts
            if item.name.startswith(('/', '\\')) or '..' in parts or ':' in item.name or not (item.isfile() or item.isdir()):
                raise ValueError('Archive contains an unsafe path or link')
            total += item.size
            if total > LIMIT:
                raise ValueError('Expanded archive exceeds 64 MiB')


def run(args):
    identifier = str(uuid.UUID(args.computer))
    credential = local_path(args.token_file).read_text().strip()
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def request(path, body=None):
        req = urllib.request.Request(args.supervisor.rstrip('/') + path,
            data=None if body is None else json.dumps(body).encode(),
            headers={'Authorization': 'Bearer ' + credential, 'Content-Type': 'application/json'})
        with opener.open(req, timeout=315) as response:
            return json.load(response)

    computer = request('/v1/computers/' + identifier)
    if computer['spec']['id'] != identifier:
        raise ValueError('Computer identity mismatch')
    if args.action == 'restore' and args.confirm != identifier:
        raise ValueError('Restore requires --confirm with the computer UUID')
    data = None
    if args.action == 'restore':
        with zipfile.ZipFile(local_path(args.archive)) as archive:
            if archive.getinfo('home.tar').file_size > LIMIT:
                raise ValueError('Archive exceeds 64 MiB')
            if archive.getinfo('computer.json').file_size > 65536:
                raise ValueError('Backup metadata exceeds the limit')
            metadata = json.loads(archive.read('computer.json'))
            if metadata['owner_id'] != computer['spec']['owner_id'] or metadata['binding'] != computer['spec']['binding']:
                raise ValueError('Backup belongs to another owner or computer scope')
            data = archive.read('home.tar')
            validate_archive(data)
    target = local_path(args.archive)
    if args.action == 'backup' and target.exists():
        raise ValueError('Backup already exists; choose a new filename')
    if computer['status'] != 'stopped':
        computer = request('/v1/computers/' + identifier + '/actions', {
            'expected_generation': computer['generation'], 'action': 'stop'})
    if computer['status'] != 'stopped':
        raise ValueError('A confirmed stop is required')
    volume = 'qunica-' + uuid.UUID(computer['supervisor_id']).hex[:8] + '-' + identifier + '-home'
    inspected = json.loads(subprocess.check_output(['docker', 'volume', 'inspect', volume]))[0]
    labels = inspected.get('Labels') or {}
    if labels.get('qunica.computer') != identifier or labels.get('qunica.supervisor') != computer['supervisor_id']:
        raise ValueError('Home volume identity mismatch')
    command = ['docker', 'run', '--rm', '-i', '--network', 'none', '--read-only', '--user', '10001:10001',
        '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges:true',
        '--mount', 'type=volume,source=' + volume + ',target=/home/agent' + (',readonly' if args.action == 'backup' else ''),
        '--entrypoint', 'python3', args.image, '-c']
    if args.action == 'backup':
        # Runtime locks and symlinks are intentionally excluded. Browser login
        # databases and project files are copied only after the confirmed stop.
        code = "import tarfile,sys; a=tarfile.open(fileobj=sys.stdout.buffer,mode='w|'); a.add('/home/agent',arcname='.',filter=lambda x: None if x.issym() or x.islnk() or '/Singleton' in x.name else x); a.close()"
        with subprocess.Popen(command + [code], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL) as process:
            data = process.stdout.read(LIMIT + 1)
            if len(data) > LIMIT:
                process.kill()
                raise ValueError('Home exceeds 64 MiB')
            if process.wait() != 0:
                raise ValueError('Home archive could not be read')
        validate_archive(data)
        target.parent.mkdir(parents=True, exist_ok=True)
        temporary = target.with_name(target.name + '.' + uuid.uuid4().hex + '.tmp')
        try:
            with zipfile.ZipFile(temporary, 'x', compression=zipfile.ZIP_DEFLATED) as archive:
                archive.writestr('computer.json', json.dumps(computer['spec']))
                archive.writestr('home.tar', data)
            os.replace(temporary, target)
        finally:
            temporary.unlink(missing_ok=True)
    else:
        # Note: Bookworm Python 3.11.2 lacks extraction filters. The archive is
        # validated before clearing home: only regular files/directories survive,
        # with no links, devices, absolute paths or traversal.
        code = "import tarfile,sys,shutil,pathlib; root=pathlib.Path('/home/agent'); [(p.unlink() if p.is_symlink() or p.is_file() else shutil.rmtree(p)) for p in root.iterdir()]; a=tarfile.open(fileobj=sys.stdin.buffer,mode='r|'); a.extractall(root)"
        subprocess.run(command + [code], input=data, check=True, stdout=subprocess.DEVNULL)
    print(args.action + ' complete; computer remains stopped, generation ' + computer['generation'])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['backup', 'restore'])
    parser.add_argument('computer', help='Computer UUID')
    parser.add_argument('archive', help='Workspace-relative ZIP path')
    parser.add_argument('--confirm', help='Required target UUID for restore')
    parser.add_argument('--supervisor', default='http://127.0.0.1:9000')
    parser.add_argument('--token-file', default='.qunica/computer-supervisor.token')
    parser.add_argument('--image', default='qunica-computer:local')
    run(parser.parse_args())
