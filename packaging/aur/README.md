# AUR packaging

`ttypo` (source build) and `ttypo-bin` (prebuilt binary) are published to
the Arch User Repository. Publishing is **automated** via
`.github/workflows/publish-aur.yml`, which fires whenever a GitHub Release
is created.

## One-time setup

1. Make an account at <https://aur.archlinux.org/>.
2. Generate (or reuse) an SSH key, add the public half to your AUR
   account's SSH Public Key field.
3. Add the **private** half as a repo secret named `AUR_SSH_PRIVATE_KEY`:

   ```sh
   gh secret set AUR_SSH_PRIVATE_KEY < ~/.ssh/aur_ed25519
   ```

4. Bootstrap each AUR repo (one push creates the empty package on AUR's
   side). From this directory:

   ```sh
   for p in ttypo ttypo-bin; do
       git clone "ssh://aur@aur.archlinux.org/$p.git" "/tmp/aur-$p"
       cp "$p/PKGBUILD" "/tmp/aur-$p/"
       cd "/tmp/aur-$p"
       updpkgsums
       makepkg --printsrcinfo > .SRCINFO
       git add PKGBUILD .SRCINFO
       git commit -m "initial $p $(grep '^pkgver=' PKGBUILD | cut -d= -f2)"
       git push
       cd -
   done
   ```

   After this, the workflow takes over.

## Normal release flow

`cargo release patch --execute` does its thing, the release workflow
publishes binaries to GH, then `publish-aur.yml` fires automatically and:

1. Bumps `pkgver` in both PKGBUILDs to the new tag version.
2. Runs `updpkgsums` against the live release tarballs to refresh sha256s.
3. Runs `makepkg --printsrcinfo` to generate `.SRCINFO`.
4. Builds the package in a clean container to verify it works.
5. Commits and pushes to the matching AUR repo.

Nothing for you to do per-release.

## Manual fallback

If the workflow fails (AUR down, package validation error, etc.), publish
by hand from this directory:

```sh
cd packaging/aur/ttypo            # or ttypo-bin
# Edit pkgver to match the new version
updpkgsums                        # refresh checksums
makepkg --printsrcinfo > .SRCINFO
makepkg -si                       # smoke-test
# Copy PKGBUILD + .SRCINFO into your aur-ttypo clone, commit, push
```
