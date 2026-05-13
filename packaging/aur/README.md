# AUR packaging

This directory holds PKGBUILDs for the Arch User Repository. The AUR is
not a git submodule - each package lives in its own AUR repo. These files
are the source of truth; copy them into the AUR repos when publishing.

## One-time setup

1. Make an account at <https://aur.archlinux.org/>.
2. Add your SSH public key under My Account.
3. Clone empty repos for each package:

   ```sh
   git clone ssh://aur@aur.archlinux.org/ttypo.git     ../aur-ttypo
   git clone ssh://aur@aur.archlinux.org/ttypo-bin.git ../aur-ttypo-bin
   ```

## Publishing a new version

For each package (`ttypo` builds from source, `ttypo-bin` uses the
`cargo-dist`-produced GitHub Release tarballs):

1. Bump `pkgver` in the PKGBUILD here.
2. Refresh checksums against the published artifacts:

   ```sh
   cd packaging/aur/ttypo      # or ttypo-bin
   updpkgsums                  # rewrites sha256sums in place
   ```

3. Generate `.SRCINFO`:

   ```sh
   makepkg --printsrcinfo > .SRCINFO
   ```

4. Smoke-test the build locally:

   ```sh
   makepkg -si
   ```

5. Copy `PKGBUILD` + `.SRCINFO` into the matching AUR clone, commit, push:

   ```sh
   cp PKGBUILD .SRCINFO ../../../../aur-ttypo/
   cd ../../../../aur-ttypo
   git add PKGBUILD .SRCINFO
   git commit -m "ttypo $NEW_VERSION"
   git push
   ```

`ttypo-bin` must be published *after* the GitHub Release exists, so the
release tarballs are downloadable for checksum verification.
