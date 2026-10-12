#!/usr/bin/env python3
"""Generic HTTP stand-in: exercise Playwright server ownership, not app behavior."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
import sys


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(f'ExampleProject ready {os.environ["PROBE_LABEL"]}'.encode())


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
