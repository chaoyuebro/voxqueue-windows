"""Version-scoped Codex desktop IPC verification; never resumes a CLI thread."""
import argparse
import ctypes
import json
import msvcrt
import os
import struct
import time
import uuid
from ctypes import wintypes


class DesktopIPC:
    def __init__(self):
        self.pipe = open(r"\\.\pipe\codex-ipc", "r+b", buffering=0)
        self.handle = wintypes.HANDLE(msvcrt.get_osfhandle(self.pipe.fileno()))
        self.peek = ctypes.windll.kernel32.PeekNamedPipe
        self.peek.argtypes = [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD,
                              ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p]
        self.peek.restype = wintypes.BOOL
        self.buffer = bytearray()
        self.client_id = "initializing-client"
        reply = self.request("initialize", {"clientType": "voxqueue-validation"}, version=0)
        if reply.get("resultType") != "success":
            raise RuntimeError("IPC initialization failed")
        self.client_id = reply["result"]["clientId"]

    def close(self):
        self.pipe.close()

    def send(self, value):
        raw = json.dumps(value, ensure_ascii=False).encode("utf-8")
        self.pipe.write(struct.pack("<I", len(raw)) + raw)

    def receive(self, deadline):
        while time.monotonic() < deadline:
            if len(self.buffer) >= 4:
                length = struct.unpack("<I", self.buffer[:4])[0]
                if length > 16 * 1024 * 1024:
                    raise RuntimeError("Unexpected frame size")
                if len(self.buffer) >= length + 4:
                    raw = bytes(self.buffer[4:length + 4])
                    del self.buffer[:length + 4]
                    return json.loads(raw)
            available = wintypes.DWORD()
            if not self.peek(self.handle, None, 0, None, ctypes.byref(available), None):
                raise ctypes.WinError()
            if available.value:
                self.buffer.extend(self.pipe.read(min(available.value, 65536)))
            else:
                time.sleep(0.03)
        raise TimeoutError("IPC response timeout; do not automatically resend")

    def request(self, method, params, version=1, target=None, timeout=15):
        request_id = str(uuid.uuid4())
        message = {"type": "request", "requestId": request_id,
                   "sourceClientId": self.client_id, "version": version,
                   "method": method, "params": params, "timeoutMs": timeout * 1000}
        if target:
            message["targetClientId"] = target
        self.send(message)
        deadline = time.monotonic() + timeout + 2
        while True:
            reply = self.receive(deadline)
            if reply.get("type") == "client-discovery-request":
                self.send({"type": "client-discovery-response", "requestId": reply["requestId"],
                           "response": {"canHandle": False}})
            if reply.get("type") == "response" and reply.get("requestId") == request_id:
                return reply


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--thread", required=True)
    parser.add_argument("--prompt", help="Explicit test text; omitted means discovery only")
    args = parser.parse_args()
    ipc = DesktopIPC()
    try:
        owner = ipc.request("thread-owner-discovery", {"hostId": "local", "conversationId": args.thread})
        print(json.dumps({"phase": "owner-discovery", "resultType": owner.get("resultType"),
                          "error": owner.get("error"), "ownerFound": bool(owner.get("handledByClientId"))}))
        if not args.prompt or owner.get("resultType") != "success":
            return
        operation = {"request": {"threadId": args.thread, "input": [{"type": "text", "text": args.prompt, "text_elements": []}],
                                 "clientUserMessageId": str(uuid.uuid4())},
                     "context": {"inheritThreadSettings": True, "attachments": [], "commentAttachments": []}}
        reply = ipc.request("thread-follower-start-turn", {"conversationId": args.thread,
                            "turnStart": operation}, version=2, target=owner["handledByClientId"], timeout=30)
        # Only the controlled test's reply is printed. Never dump stream snapshots.
        print(json.dumps({"phase": "start-turn", "resultType": reply.get("resultType"),
                          "error": reply.get("error"), "result": reply.get("result")}, ensure_ascii=False))
    finally:
        ipc.close()


if __name__ == "__main__":
    main()
