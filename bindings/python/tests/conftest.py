"""Build the mock-enabled cdylib, point MEDIUS_LIB at it, and put the package on
the path before anything imports `medius`."""

import os
import subprocess
import sys
from pathlib import Path

_HERE = Path(__file__).resolve()
_PKG_ROOT = _HERE.parents[1]      # bindings/python
_WORKSPACE = _HERE.parents[3]     # medius workspace root

_LIB_NAMES = {
    "linux": "libmedius_capi.so",
    "darwin": "libmedius_capi.dylib",
    "win32": "medius_capi.dll",
}


def _build_mock_lib() -> Path:
    subprocess.run(
        ["cargo", "build", "-p", "medius-capi", "--features", "mock"],
        cwd=str(_WORKSPACE),
        check=True,
    )
    name = _LIB_NAMES.get(sys.platform, "libmedius_capi.so")
    return _WORKSPACE / "target" / "debug" / name


_lib = _build_mock_lib()
os.environ["MEDIUS_LIB"] = str(_lib)
if str(_PKG_ROOT) not in sys.path:
    sys.path.insert(0, str(_PKG_ROOT))
