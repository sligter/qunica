"""Offline tests of the address boundary; no real metadata requests."""
import importlib.util
import unittest
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('egress', Path(__file__).resolve().parent.parent / 'docker/computer-egress.py')
egress = importlib.util.module_from_spec(spec)
spec.loader.exec_module(egress)

class Boundary(unittest.TestCase):
    def test_public_only_by_default(self):
        for address in ['127.0.0.1','::1','169.254.169.254','168.63.129.16','10.1.2.3','172.17.0.1','192.168.1.1','::ffff:127.0.0.1','fc00::1','0.0.0.0','224.0.0.1']:
            with self.subTest(address=address): self.assertFalse(egress.permitted(address))
        self.assertTrue(egress.permitted('1.1.1.1'))

    def test_explicit_internal_allow_does_not_override_control_deny(self):
        import ipaddress
        with patch.object(egress,'ALLOW',[ipaddress.ip_network('10.0.0.0/8')]), patch.object(egress,'DENY',[ipaddress.ip_network('10.0.1.0/24')]):
            self.assertTrue(egress.permitted('10.0.2.5'))
            self.assertFalse(egress.permitted('10.0.1.5'))
            self.assertFalse(egress.permitted('169.254.169.254'))

    def test_mixed_dns_and_non_http_ports_fail_before_connect(self):
        import socket
        addresses=[(socket.AF_INET,socket.SOCK_STREAM,6,'',('1.1.1.1',80)),(socket.AF_INET,socket.SOCK_STREAM,6,'',('127.0.0.1',80))]
        with patch.object(socket,'getaddrinfo',return_value=addresses),patch.object(socket,'socket') as connect:
            with self.assertRaises(ValueError):egress.connect_public('mixed.test',80)
            with self.assertRaises(ValueError):egress.connect_public('mixed.test',22)
            connect.assert_not_called()

if __name__=='__main__': unittest.main()
