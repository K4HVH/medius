"""Conan recipe for the medius C/C++ bindings.

Builds the medius_capi C library with cargo and packages it together with the C
header and the header-only C++ wrapper. Intended for ConanCenter or a private
remote: `source()` fetches the tagged release, so a build needs no checkout.

    conan create . -o medius/*:mock=False -o medius/*:flash=False
"""

import os

from conan import ConanFile
from conan.tools.files import copy, get


class MediusConan(ConanFile):
    name = "medius"
    version = "2.2.0"
    license = "MIT"
    homepage = "https://github.com/K4HVH/medius"
    description = "C/C++ bindings for the medius transparent mouse passthrough box"
    topics = ("hid", "mouse", "serial", "makcu", "ffi")
    settings = "os", "arch", "compiler", "build_type"
    options = {"mock": [True, False], "flash": [True, False]}
    default_options = {"mock": False, "flash": False}

    def source(self):
        get(
            self,
            "https://github.com/K4HVH/medius/archive/refs/tags/v{}.tar.gz".format(self.version),
            strip_root=True,
        )

    def build(self):
        features = []
        if self.options.mock:
            features.append("mock")
        if self.options.flash:
            features.append("flash")
        cmd = "cargo build --release -p medius-capi"
        if features:
            cmd += " --features " + ",".join(features)
        self.run(cmd, cwd=self.source_folder)

    def package(self):
        inc = os.path.join(self.package_folder, "include")
        copy(self, "medius.h", os.path.join(self.source_folder, "medius-capi", "include"), inc)
        copy(self, "*.hpp", os.path.join(self.source_folder, "bindings", "cpp", "include"), inc)
        libdir = os.path.join(self.source_folder, "target", "release")
        out = os.path.join(self.package_folder, "lib")
        for pattern in (
            "*medius_capi.so",
            "*medius_capi.dylib",
            "*medius_capi.a",
            "medius_capi.dll.lib",
        ):
            copy(self, pattern, libdir, out, keep_path=False)
        copy(self, "medius_capi.dll", libdir, os.path.join(self.package_folder, "bin"), keep_path=False)

    def package_info(self):
        self.cpp_info.libs = ["medius_capi"]
        if self.options.mock:
            self.cpp_info.defines.append("MEDIUS_FEATURE_MOCK")
        if self.options.flash:
            self.cpp_info.defines.append("MEDIUS_FEATURE_FLASH")
        if self.settings.os == "Linux":
            self.cpp_info.system_libs = ["pthread", "dl", "m"]
