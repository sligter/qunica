import importlib.util
import io
from pathlib import Path
import tarfile
import unittest

spec=importlib.util.spec_from_file_location('home',Path(__file__).resolve().parent/'computer-home.py')
home=importlib.util.module_from_spec(spec)
spec.loader.exec_module(home)

def archive(name,kind=tarfile.REGTYPE):
    buffer=io.BytesIO()
    with tarfile.open(fileobj=buffer,mode='w') as output:
        entry=tarfile.TarInfo(name);entry.type=kind
        if kind==tarfile.SYMTYPE:entry.linkname='/tmp/escape'
        output.addfile(entry)
    return buffer.getvalue()

class ArchiveBoundary(unittest.TestCase):
    def test_regular_files_are_accepted(self):home.validate_archive(archive('./workspace/file.txt'))
    def test_parent_absolute_and_windows_paths_are_rejected(self):
        for path in ['../escape','/absolute','C:/absolute','a/../../escape','a\\..\\escape']:
            with self.subTest(path=path),self.assertRaises(ValueError):home.validate_archive(archive(path))
    def test_links_and_devices_are_rejected(self):
        for kind in [tarfile.SYMTYPE,tarfile.LNKTYPE,tarfile.CHRTYPE,tarfile.FIFOTYPE]:
            with self.subTest(kind=kind),self.assertRaises(ValueError):home.validate_archive(archive('workspace/link',kind))
    def test_workspace_paths_cannot_escape(self):
        for path in ['../backup.zip','C:/backup.zip']:
            with self.subTest(path=path),self.assertRaises(ValueError):home.local_path(path)

if __name__=='__main__':unittest.main()
