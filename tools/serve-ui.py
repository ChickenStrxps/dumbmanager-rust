"""Layout preview only. Native features require the desktop executable."""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class PreviewHandler(SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


if __name__ == "__main__":
    root = Path(__file__).resolve().parents[1] / "web"
    handler = partial(PreviewHandler, directory=str(root))
    print("UI preview: http://127.0.0.1:43179 (no native operations)", flush=True)
    ThreadingHTTPServer(("127.0.0.1", 43179), handler).serve_forever()
