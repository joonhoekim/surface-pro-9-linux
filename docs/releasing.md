# Releasing

The repository has one version, `X.Y.Z` ([Semantic Versioning](https://semver.org/spec/v2.0.0.html)), and a
release is the tag `vX.Y.Z` on `main`. Before 1.0.0 a minor version may change behaviour or options.

## Where the version lives

| What | Takes the version from |
|---|---|
| `[workspace.package] version` in [`helper/Cargo.toml`](../helper/Cargo.toml) | The one place it is written |
| The helper crates (`sp9-helperd`, `sp9-ctl`, `sp9-settings`, `sp9-digitizer`, `sp9-digitizerd`) | `version.workspace = true` |
| `--version` of `sp9-ctl`, `sp9-helperd`, `sp9-digitizerd`, `sp9-settings`, and the `Version` D-Bus properties | [`helper/version-build.rs`](../helper/version-build.rs): the crate version, plus `git describe` in parentheses unless the build is exactly at the tag `v<version>` (`0.1.0 (v0.1.0-4-g1a2b3c4-dirty)`; before the first tag the commit alone). Without a repository (a tarball, the Nix sandbox) the version alone, or `SP9_GIT_DESCRIBE` from the environment |
| Nix packages `sp9-helper`, `sp9-digitizerd`, `surface-pen-tools` | [`nix/version.nix`](../nix/version.nix), which reads `helper/Cargo.toml`. The packages take an optional `gitDescribe` argument for the suffix |
| `kernel`, `libcamera` packages | Their upstream versions (7.3.4, 0.8.0); the patches on them are part of the release |

## Cutting a release

1. On an up-to-date `main` with a clean tree, run the checks of [CONTRIBUTING.md](../CONTRIBUTING.md):
   `nix run nixpkgs#reuse -- lint`, `tools/public-scan.sh`, `cargo test` in `helper/`, and
   `nix flake check` (CI runs the same).
2. Set the version in `helper/Cargo.toml` and update the lock file: `cargo update --workspace` in `helper/`
   (only the workspace's own entries change).
3. In [CHANGELOG.md](../CHANGELOG.md) rename `[Unreleased]` to `[X.Y.Z] - YYYY-MM-DD`, start a new empty
   `[Unreleased]` above it, and update the comparison links at the bottom.
4. Bring the counts in [README.md](../README.md) (patches, test tally) in line with the tree if they moved.
5. Commit (`Release X.Y.Z`), tag with an annotated tag and push both:

   ```sh
   git tag -a vX.Y.Z -m "X.Y.Z"
   git push origin main vX.Y.Z
   ```

6. Check that a build from the tag reports the bare version: `cargo run -p sp9-ctl -- --version` prints
   `sp9-ctl X.Y.Z`.
7. Create the GitHub release from the tag with the CHANGELOG section as its text.

A fix to a release goes to `main` first and is released as `X.Y.Z+1`. There are no release branches.

## Packages and repositories

The tag also publishes the packages ([packaging/README.md](../packaging/README.md)). The Packages workflow
([`.github/workflows/packages.yml`](../.github/workflows/packages.yml)) runs on the pushed tag `vX.Y.Z`:

1. It refuses a tag that is not `v` plus the version in `helper/Cargo.toml`. (Before tagging, also add the version
   with its date to the AppStream metainfo in `helper/data/`, `<releases><release version="X.Y.Z" date="YYYY-MM-DD"/>`
   (newest first): a release without a date fails AppStream validation, and with it lintian and the install test.)
2. It builds the source tarball (the tree plus the vendored Rust crates), the Debian and Ubuntu packages of every
   release in [`packaging/targets.tsv`](../packaging/targets.tsv), the Arch packages and `sp9-libcamera`, runs
   lintian, namcap and the install tests, and stops at the first failure: nothing is published then.
3. It signs the apt and pacman repositories with the subkey in the secret `SP9_REPO_SIGNING_KEY` and creates the
   GitHub release, with the CHANGELOG section as its text (that is step 7 above), the packages, the source tarball,
   the filled-in PKGBUILD, `SHA256SUMS` with its signature and `pages-packages.tar.gz`. If the release already exists,
   the files are added to it.
4. Its success starts the Website workflow, which deploys the site with the repositories under `/packages/`.

After it: `apt update` and `pacman -Sy` against the site see the new version (section 9 of
[install-debian-ubuntu.md](install-debian-ubuntu.md) or [install-arch.md](install-arch.md) on a test machine, or `apt-cache policy sp9-helper` in a container).

A failed run is fixed on `main` and the release is tagged again with the next patch version; a published tag is not
moved. A run can be repeated (Actions, "Re-run jobs") when it failed for reasons outside the tree.

### Once: the signing key

Before the first release:

1. `tools/repo-key.sh DIR` on a trusted machine, with `DIR` outside the repository. It creates a certify-only
   Ed25519 primary key and an Ed25519 signing subkey (2 years), asks for a passphrase for the primary key, and writes
   the public key, the subkey for CI and a backup of both.
2. Copy `DIR/sp9-archive-keyring.asc` to `packaging/keyring/` and put the fingerprint into
   [SECURITY.md](../SECURITY.md#package-signing); commit both.
3. `gh secret set SP9_REPO_SIGNING_KEY < DIR/signing-subkey-secret.asc`, then delete that file.
4. Keep `DIR` (the primary key) offline.
5. GitHub Pages on, source "GitHub Actions" (Settings, Pages), as the website already needs.

### Renewing or replacing the subkey

The subkey expires after 2 years; renew it a few months before, so that the keyring packages carrying the new subkey
reach users while the old one still works:

1. `tools/repo-key.sh --renew-subkey DIR` adds a new signing subkey (the primary key's passphrase is asked).
2. Copy the new `sp9-archive-keyring.asc` to `packaging/keyring/`, commit, release: the keyring packages now carry
   both subkeys.
3. `gh secret set SP9_REPO_SIGNING_KEY < DIR/signing-subkey-secret.asc`; the next release is signed with the new
   subkey.

If the secret leaks: revoke the subkey with the primary key (`gpg --homedir DIR/gnupg --edit-key FINGERPRINT`,
`key N`, `revkey`), renew as above, release at once, and say so in a security advisory. If the primary key is lost
or leaks, a new key has to be distributed the way the first one was (section 2 of [install-debian-ubuntu.md](install-debian-ubuntu.md) and [install-arch.md](install-arch.md)), and users have
to trust it again.
