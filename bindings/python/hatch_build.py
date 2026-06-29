"""Hatchling build hook: build the medius_capi cdylib and bundle it in the wheel.

Runs `cargo build --release -p medius-capi` from the workspace root and copies
the produced shared library into the package so the wheel ships it. Set
MEDIUS_SKIP_CARGO=1 to skip the cargo step and use a library already present in
the target dir (e.g. one a CI before-build step produced).
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path

from hatchling.builders.hooks.plugin.interface import BuildHookInterface

_LIB_NAMES = {
    "linux": "libmedius_capi.so",
    "darwin": "libmedius_capi.dylib",
    "win32": "medius_capi.dll",
}


def _lib_name() -> str:
    return _LIB_NAMES.get(sys.platform, "libmedius_capi.so")


class CustomBuildHook(BuildHookInterface):
    def initialize(self, version, build_data):
        # The bundled cdylib is platform-specific, but ctypes doesn't link the
        # CPython ABI, so one py3-none-<platform> wheel serves every Python 3.x.
        build_data["pure_python"] = False
        try:
            from packaging.tags import sys_tags

            build_data["tag"] = "py3-none-{}".format(next(iter(sys_tags())).platform)
        except Exception:
            build_data["infer_tag"] = True

        root = Path(self.root)              # bindings/python
        workspace = root.parent.parent      # medius workspace root
        pkg = root / "medius"
        name = _lib_name()

        if not os.environ.get("MEDIUS_SKIP_CARGO"):
            subprocess.run(
                ["cargo", "build", "--release", "-p", "medius-capi"],
                cwd=str(workspace),
                check=True,
            )

        src = workspace / "target" / "release" / name
        if not src.exists():
            debug = workspace / "target" / "debug" / name
            if debug.exists():
                src = debug
        if not src.exists():
            raise FileNotFoundError(
                "could not find {} under {}/target; build medius-capi first".format(name, workspace)
            )

        dest = pkg / name
        shutil.copy2(src, dest)

        rel = "medius/{}".format(name)
        build_data["artifacts"].append(rel)
        build_data["force_include"][str(dest)] = rel
