# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# The project version, read from the one place it is written: `[workspace.package] version` in
# helper/Cargo.toml. Releases are tagged v<this> (docs/releasing.md). The packages of this
# repository take it as their version; the kernel and libcamera keep their upstream versions.
(builtins.fromTOML (builtins.readFile ../helper/Cargo.toml)).workspace.package.version
