"""Zero-dependency Tomoyo Python 3 reference plugin (JSON-RPC 2.0 over stdio)."""

import hashlib
import json
import sys


def handle_query(ctx: dict) -> list:
    text = (ctx.get("args") or "").strip()
    if not text:
        return [
            {
                "id": "py:hint",
                "provider_id": "dev.tomoyo.example-python",
                "title": "Type text after `py ` or `hash ` to compute SHA-256 / MD5 / stats",
                "subtitle": "Zero-dependency Python plugin · isolated via Windows Job Object",
                "category": "plugin",
                "score": 9000,
                "icon": {"kind": "monogram", "value": "PY"},
                "badge": "PYTHON",
                "actions": [],
            }
        ]

    raw = text.encode("utf-8")
    sha256 = hashlib.sha256(raw).hexdigest()
    md5 = hashlib.md5(raw).hexdigest()
    stats = f"{len(text)} chars · {len(raw)} bytes · {len(text.split())} words"

    def item(item_id: str, title: str, subtitle: str, badge: str, score: int) -> dict:
        return {
            "id": item_id,
            "provider_id": "dev.tomoyo.example-python",
            "title": title,
            "subtitle": subtitle,
            "category": "plugin",
            "score": score,
            "icon": {"kind": "monogram", "value": "PY"},
            "badge": badge,
            "copy_text": title,
            "actions": [
                {
                    "id": "copy",
                    "label": "Copy Value",
                    "shortcut": "Enter",
                    "kind": {"type": "copy_text", "text": title, "paste_immediately": False},
                }
            ],
        }

    return [
        item("py:sha256", sha256, f"SHA-256 of `{text[:40]}` (Enter to copy)", "SHA256", 9600),
        item("py:md5", md5, f"MD5 of `{text[:40]}` (Enter to copy)", "MD5", 9500),
        item("py:stats", stats, "UTF-8 Text Statistics (Enter to copy)", "STATS", 9400),
    ]


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except Exception as exc:
            sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": 0, "error": {"code": -32700, "message": str(exc)}}) + "\n")
            sys.stdout.flush()
            continue

        req_id = req.get("id", 0)
        method = req.get("method", "")
        params = req.get("params", {})

        if method == "tomoyo/handshake":
            resp = {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"protocol_version": 1, "name": "Python Quick Hash", "version": "0.1.0"},
            }
        elif method == "tomoyo/query":
            resp = {"jsonrpc": "2.0", "id": req_id, "result": {"items": handle_query(params)}}
        elif method == "tomoyo/execute":
            resp = {"jsonrpc": "2.0", "id": req_id, "result": {"hide_window": True}}
        elif method == "tomoyo/shutdown":
            resp = {"jsonrpc": "2.0", "id": req_id, "result": {"ok": True}}
            sys.stdout.write(json.dumps(resp) + "\n")
            sys.stdout.flush()
            break
        else:
            resp = {
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {"code": -32601, "message": f"unknown method: {method}"},
            }

        sys.stdout.write(json.dumps(resp) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
