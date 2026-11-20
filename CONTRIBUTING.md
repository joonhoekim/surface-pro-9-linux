# Contributing

Issues are for problems you can reproduce on a Surface Pro 9 with an Intel CPU (SKU `Surface_Pro_9_2038`); questions,
ideas and device reports go to [Discussions](https://github.com/joonhoekim/surface-pro-9-linux/discussions). Other Surface models, and linux-surface's own patches
and packages, belong to [linux-surface](https://github.com/linux-surface/linux-surface).

Every result in this repository comes from one unit, so **a test run on another unit is as useful as code**. Pick a
test from [docs/test-plan.md](docs/test-plan.md), run it, and report it with the "Test result" issue form, which
asks for the columns of [docs/test-results.md](docs/test-results.md): ID, result, evidence, kernel, config, values.
A wider "what works on my unit" report goes to
[Device reports](https://github.com/joonhoekim/surface-pro-9-linux/discussions/categories/device-reports). Say which commit you ran and what you measured or saw.

Before you post anything, remove serial numbers, MAC and Bluetooth addresses, disk UUIDs, network names, and host and
user names. The post-install check (`tools/linux/post-install-check.sh`) masks addresses, UUIDs and serial lines by
pattern; look over its output all the same.

## Pull requests

- One change per pull request; say what it is for and how it was tested on the device, or that it was not.
- **"Works" needs evidence.** Documentation says a part works only after a measurement (logs, numbers, counters) or
  an observation (a person saw, heard or felt it). A driver that probed, or a device node that exists, is not
  evidence. Mark statements as measured, observed, inferred or unverified, as the existing documents do.
- **Kernel patches** go in `kernel/patches/` as `git format-patch` output on the base in
  [kernel/PROVENANCE.md](kernel/PROVENANCE.md), with a row there (origin, status, evidence). A patch taken from
  someone else keeps its `From:` and every `Signed-off-by:`. Patches meant for upstream Linux follow the kernel's
  [submitting-patches](https://docs.kernel.org/process/submitting-patches.html) rules; a fix that belongs in
  linux-surface is better sent there.
- **Nothing taken from firmware or Windows drivers.** No firmware files, no driver binaries, no text copied from
  them. Facts read from the running Windows system (registry values, device properties, measured behaviour) are
  fine, stated in your own words.
- **No device-unique data**: serials, addresses, UUIDs, the ACPI `MSDM` table (it holds the Windows product key).
  Model-level data, the same on every unit, is fine.
- Run before you push: `nix run nixpkgs#reuse -- lint` (every file has a licence), `tools/public-scan.sh` (no
  home paths, addresses or other private data), and for the helper `cargo test` in `helper/`. The website
  (`site/`) is built and checked by CI; [site/README.md](site/README.md) says how to build it locally.

## Licensing of contributions (inbound = outbound)

A contribution is licensed under the licence of the file it changes, as given by that file's SPDX header (or by
[REUSE.toml](REUSE.toml) for files without one); [LICENSE](LICENSE) and [NOTICE](NOTICE) have the overview. A new file
takes the licence of the component it belongs to and starts with an SPDX header:

| Path | Licence |
|---|---|
| `kernel/patches/`, `kernel/config/` | GPL-2.0-only, as the Linux kernel |
| `helper/` (device helper, touch and pen daemon, settings app) | GPL-3.0-or-later |
| `userspace/`, `tools/`, `nix/`, `site/`, `flake.nix` | MIT, except patches to other projects (`userspace/iptsd/`, `userspace/libcamera/`), which take that project's licence as REUSE.toml records |
| Documentation (`*.md`, `docs/`) | CC-BY-SA-4.0; code and commands quoted in it may also be used under MIT |

The header is two lines in the file's comment syntax:

    SPDX-License-Identifier: MIT
    Copyright (C) 2026 Your Name

There is no contributor licence agreement.

## Developer Certificate of Origin

Issues, discussions and test reports need nothing of this; it is only for commits.

Some of the kernel patches here also go to the upstream Linux kernel, so this repository takes its sign-off rule from
the kernel's
[submitting-patches](https://docs.kernel.org/process/submitting-patches.html#sign-your-work-the-developer-s-certificate-of-origin).
Add a `Signed-off-by:` line to every commit in a pull request, with the same name and e-mail as the commit's author.
It states that you may contribute the change under the file's licence, as set out in the
[Developer Certificate of Origin 1.1](https://developercertificate.org/):

    Signed-off-by: Your Name <you@example.org>

Use a name you are known by, such as the one on your GitHub profile; it does not have to be your legal name.
`git commit -s` adds the line.

If the DCO check on your pull request fails, there is no need to open a new one. Add the line to the commits you have
already made and push them to the same branch:

    git rebase --signoff BASE        # BASE: the branch your pull request targets, for example origin/main
    git push --force-with-lease

Dependency updates opened by Dependabot are exempt: they change version numbers only.

## Names

Forks and modified builds use their own name, icons and D-Bus, polkit and application IDs; see
[TRADEMARKS.md](TRADEMARKS.md).
