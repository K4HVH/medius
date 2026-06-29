"""Hatchling build hook: bundle the medius_capi cdylib into the wheel.

Two layouts are supported. In the dev tree the Rust workspace is two levels up
(`../..`). In an sdist the workspace is vendored under `_rust/` so the wheel can
build with no access to the rest of the repo (this is what makes cibuildwheel
work from the sdist). The sdist build vendors those sources; the wheel build
runs `cargo build --release -p medius-capi` in whichever workspace it finds and
copies the cdylib into the package. Set MEDIUS_SKIP_CARGO=1 to reuse a lib
already in the target dir.
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

# Workspace entries the C ABI build needs. Directories are copied recursively
# (minus target/); files are copied if present.
_VENDOR_DIRS = ["src", "examples", "medius-capi/src", "medius-capi/include"]
_VENDOR_FILES = [
    "Cargo.toml",
    "Cargo.lock",
    "LICENSE",
    "medius-capi/Cargo.toml",
    "medius-capi/cbindgen.toml",
]


def _lib_name() -> str:
    return _LIB_NAMES.get(sys.platform, "libmedius_capi.so")


def _locate_workspace(root: Path) -> Path:
    vendored = root / "_rust"
    if (vendored / "Cargo.toml").exists():
        return vendored
    return root.parent.parent


class CustomBuildHook(BuildHookInterface):
    def initialize(self, version, build_data):
        if self.target_name == "sdist":
            self._vendor_rust(build_data)
            return

        # The bundled cdylib is platform-specific, but ctypes doesn't link the
        # CPython ABI, so one py3-none-<platform> wheel serves every Python 3.x.
        build_data["pure_python"] = False
        try:
            from packaging.tags import sys_tags

            build_data["tag"] = "py3-none-{}".format(next(iter(sys_tags())).platform)
        except Exception:
            build_data["infer_tag"] = True

        root = Path(self.root)
        workspace = _locate_workspace(root)
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

    def _vendor_rust(self, build_data):
        """Force-include the Rust workspace into the sdist under `_rust/`."""
        workspace = Path(self.root).parent.parent
        for rel_dir in _VENDOR_DIRS:
            base = workspace / rel_dir
            if not base.is_dir():
                continue
            for path in base.rglob("*"):
                if path.is_file() and "target" not in path.relative_to(workspace).parts:
                    rel = path.relative_to(workspace)
                    build_data["force_include"][str(path)] = "_rust/{}".format(rel.as_posix())
        for rel_file in _VENDOR_FILES:
            path = workspace / rel_file
            if path.is_file():
                build_data["force_include"][str(path)] = "_rust/{}".format(rel_file)
