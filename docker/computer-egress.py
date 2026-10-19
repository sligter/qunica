"""Per-computer HTTP proxy. Connect to the validated IP, never resolve twice."""
import http.client
import ipaddress
import os
import select
import socket
import socketserver
from urllib.parse import urlsplit
from http.server import BaseHTTPRequestHandler

ALLOW = [ipaddress.ip_network(x.strip()) for x in os.getenv('QUNICA_EGRESS_ALLOW_CIDRS', '').split(',') if x.strip()]
DENY = [ipaddress.ip_network('168.63.129.16/32')] + [ipaddress.ip_network(x.strip()) for x in os.getenv('QUNICA_EGRESS_DENY_CIDRS', '').split(',') if x.strip()]
BACKEND = urlsplit(os.getenv('QUNICA_PROXY_BACKEND', 'http://qunica:8766'))
HOP = {'connection', 'proxy-connection', 'proxy-authorization', 'keep-alive', 'transfer-encoding', 'upgrade', 'te', 'trailer'}


def permitted(address):
    ip = ipaddress.ip_address(address)
    if getattr(ip, 'ipv4_mapped', None):
        ip = ip.ipv4_mapped
    if ip.is_loopback or ip.is_link_local or ip.is_multicast or ip.is_unspecified or any(ip in n for n in DENY if ip.version == n.version):
        return False
    return ip.is_global or any(ip in n for n in ALLOW if ip.version == n.version)


def connect_public(host, port):
    if port not in (80, 443):
        raise ValueError('port denied')
    addresses = socket.getaddrinfo(host, port, type=socket.SOCK_STREAM)
    if not addresses or any(not permitted(x[4][0]) for x in addresses):
        raise ValueError('address denied')
    # Note: DNS rebinding cannot change the destination after authorization.
    family, kind, proto, _, address = addresses[0]
    conn = socket.socket(family, kind, proto)
    conn.settimeout(30)
    try:
        conn.connect(address)
    except Exception:
        conn.close()
        raise
    return conn


class Proxy(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *_):
        pass  # URLs and credentials never enter the container log.

    def do_CONNECT(self):
        try:
            target = urlsplit('//' + self.path)
            if target.username or target.password or target.path:
                raise ValueError('invalid authority')
            upstream = connect_public(target.hostname, target.port or 443)
        except Exception:
            self.send_error(403, 'Destination denied')
            return
        self.send_response(200)
        self.end_headers()
        self.close_connection = True
        with upstream:
            peers = [self.connection, upstream]
            while True:
                ready, _, _ = select.select(peers, [], [], 60)
                if not ready:
                    break
                for src in ready:
                    data = src.recv(65536)
                    if not data:
                        return
                    (upstream if src is self.connection else self.connection).sendall(data)

    def forward(self):
        conn = None
        try:
            if self.headers.get('Transfer-Encoding') or self.headers.get('Upgrade'):
                raise ValueError('unsupported framing')
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 <= length <= 8 * 1024 * 1024:
                raise ValueError('body limit')
            target = urlsplit(self.path)
            bridge = target.path in ('/qunica/model/v1/chat/completions', '/qunica/notes')
            if bridge:
                if self.command != 'POST' or target.query:
                    raise ValueError('bridge route denied')
                path = '/internal/computer/' + ('model/chat/completions' if target.path.endswith('completions') else 'notes')
                conn = http.client.HTTPConnection(BACKEND.hostname, BACKEND.port or 8000, timeout=310)
                host = BACKEND.netloc
            else:
                if target.scheme != 'http' or not target.hostname or target.username or target.password:
                    raise ValueError('invalid URL')
                path = target.path or '/'
                if target.query:
                    path += '?' + target.query
                conn = http.client.HTTPConnection(target.hostname, target.port or 80, timeout=60)
                conn.sock = connect_public(target.hostname, target.port or 80)
                host = target.netloc
            headers = {k: v for k, v in self.headers.items() if k.lower() not in HOP | {'host'}}
            headers['Host'] = host
            headers['Connection'] = 'close'
            if bridge:
                # These values come only from this trusted sidecar, not the
                # caller. The derived proof cannot authorize Supervisor routes.
                headers = {k: v for k, v in headers.items() if not k.lower().startswith('x-qunica-')}
                headers['x-qunica-computer'] = os.environ['QUNICA_COMPUTER_ID']
                headers['x-qunica-generation'] = os.environ['QUNICA_COMPUTER_GENERATION']
                headers['x-qunica-egress'] = os.environ['QUNICA_EGRESS_ATTESTATION']
            conn.request(self.command, path, self.rfile.read(length), headers)
            response = conn.getresponse()
            self.send_response(response.status)
            for key, value in response.getheaders():
                if key.lower() not in HOP | {'content-length', 'set-cookie', 'server'}:
                    self.send_header(key, value)
            self.send_header('Connection', 'close')
            self.end_headers()
            self.close_connection = True
            while chunk := response.read1(65536):
                self.wfile.write(chunk)
                self.wfile.flush()
        except Exception:
            self.close_connection = True
            if conn is None or conn.sock is None:
                self.send_error(403, 'Destination denied')
        finally:
            if conn:
                conn.close()

    do_GET = do_HEAD = do_POST = do_PUT = do_PATCH = do_DELETE = forward


if __name__ == '__main__':
    class Server(socketserver.ThreadingMixIn, socketserver.TCPServer):
        allow_reuse_address = True
        daemon_threads = True
        request_queue_size = 64
    with Server(('0.0.0.0', 3128), Proxy) as server:
        server.serve_forever()
