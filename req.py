http = """GET /api/test HTTP/1.1
Host: 127.0.0.1:8100
User-Agent: curl/7.87.0
Accept: */*

"""

import socket
import sys

with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.connect(("127.0.0.1", 8100))
    sys.stdin.read(1)
    sock.send(http.encode())
    print(sock.recv(1024).decode())
