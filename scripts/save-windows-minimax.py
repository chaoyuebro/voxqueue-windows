"""Read a MiniMax API key from stdin into the current Windows user's Credential Manager.

Example: Get-Clipboard -Raw | python scripts/save-windows-minimax.py
The key is never accepted as a command argument or printed.
"""

import ctypes
from ctypes import wintypes
import sys


class FILETIME(ctypes.Structure):
    _fields_ = [("low", wintypes.DWORD), ("high", wintypes.DWORD)]


class CREDENTIALW(ctypes.Structure):
    _fields_ = [
        ("flags", wintypes.DWORD), ("type", wintypes.DWORD),
        ("target_name", wintypes.LPWSTR), ("comment", wintypes.LPWSTR),
        ("last_written", FILETIME), ("blob_size", wintypes.DWORD),
        ("blob", ctypes.POINTER(ctypes.c_ubyte)), ("persist", wintypes.DWORD),
        ("attribute_count", wintypes.DWORD), ("attributes", ctypes.c_void_p),
        ("target_alias", wintypes.LPWSTR), ("user_name", wintypes.LPWSTR),
    ]


def main() -> None:
    if sys.platform != "win32":
        raise RuntimeError("Windows is required")
    key = sys.stdin.read().strip()
    if not 20 <= len(key) <= 2048 or not key.isascii() or not key.isprintable() or any(c.isspace() for c in key):
        raise ValueError("invalid API key format")
    encoded = bytearray(key.encode("utf-16-le"))
    blob = (ctypes.c_ubyte * len(encoded)).from_buffer(encoded)
    credential = CREDENTIALW()
    credential.type = 1
    credential.target_name = "EasyCodexInput/MINIMAX_API_KEY"
    credential.user_name = "MiniMax"
    credential.blob_size = len(encoded)
    credential.blob = ctypes.cast(blob, ctypes.POINTER(ctypes.c_ubyte))
    credential.persist = 2
    library = ctypes.WinDLL("Advapi32.dll", use_last_error=True)
    library.CredWriteW.argtypes = [ctypes.POINTER(CREDENTIALW), wintypes.DWORD]
    library.CredWriteW.restype = wintypes.BOOL
    try:
        if not library.CredWriteW(ctypes.byref(credential), 0):
            raise OSError(ctypes.get_last_error(), "credential write failed")
    finally:
        ctypes.memset(ctypes.addressof(blob), 0, len(encoded))
    print("MiniMax key saved in Windows Credential Manager; value not displayed.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError) as error:
        print(f"Save failed: {error}", file=sys.stderr)
        sys.exit(1)
