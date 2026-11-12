"""Helper (not a story file): a fake OpenAI-compatible chat server for the LLM-judge e2e test (US-66).

Synthetic only, no model. An answer request gets a fixed answer (with a thinking block the judge
must drop). A judge request gets FAIL when its criterion says "exactly one", else PASS.
Usage: fake-judge.py <port-file> <request-log>   (writes the port once it listens; logs one line per request)
"""

from __future__ import annotations

import json
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT_FILE, LOG = sys.argv[1], sys.argv[2]


class Handler(BaseHTTPRequestHandler):
    def do_POST(self) -> None:
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        prompt = body["messages"][-1]["content"]
        if "Criterion:" in prompt:
            criterion = prompt.split("Criterion:", 1)[1].split("\n", 1)[0]
            content = json.dumps({"verdict": "FAIL" if "exactly one" in criterion else "PASS", "evidence": "fake judge"})
        else:
            content = "<think>synthetic plan</think>I would run `careerbrew review` and propose one next step."
        with open(LOG, "a", encoding="utf-8") as fh:
            fh.write(f"{self.path} {'judge' if 'Criterion:' in prompt else 'answer'} {body.get('model')}\n")
        out = json.dumps({"choices": [{"index": 0, "finish_reason": "stop", "message": {"role": "assistant", "content": content}}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)

    def log_message(self, format: str, *args: object) -> None:  # noqa: A002 (the base class names it)
        pass


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
with open(PORT_FILE + ".tmp", "w", encoding="utf-8") as fh:
    fh.write(str(server.server_port))
os.replace(PORT_FILE + ".tmp", PORT_FILE)
server.serve_forever()
