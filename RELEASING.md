# Releasing linkup

This repo uses [release-please](https://github.com/googleapis/release-please) to
manage versions and changelog, and
[dist](https://opensource.axo.dev/cargo-dist/) to build and publish artifacts.
Releases are driven by **conventional commits in PR titles**. We squash-merge,
so the PR title becomes the commit on `main`.

## Conventional commit prefixes

PR titles must start with one of:

| Prefix                                                                              | Meaning                        | Effect on release           |
| ----------------------------------------------------------------------------------- | ------------------------------ | --------------------------- |
| `feat:`                                                                             | New user-facing capability     | Minor bump (4.0.0 -> 4.1.0) |
| `fix:`                                                                              | Bug fix                        | Patch bump (4.0.0 -> 4.0.1) |
| `feat!:` / `fix!:`                                                                  | Breaking change (note the `!`) | Major bump (4.0.0 -> 5.0.0) |
| `chore:` / `docs:` / `refactor:` / `perf:` / `test:` / `ci:` / `build:` / `revert:` | Maintenance                    | No release bump             |

The `lint-pr-title` workflow rejects PRs that don't match.

## Cutting the next release from `main`

1. Merge PRs into `main` as usual. Each merge updates an open "chore: release
   X.Y.Z" PR maintained by release-please.
2. When ready to promote to stable, **merge the release-please PR**. This bumps
   `[workspace.package].version`, tags the commit (`X.Y.Z`, no `v` prefix), and
   creates a GitHub release with a generated changelog.
3. The tag push triggers `release.yml` (dist), which:
   - Builds the cross-target matrix.
   - Uploads `linkup-{target}.tar.gz` assets.
   - Opens a PR against `mentimeter/homebrew-mentimeter` to bump the formula.
4. Merge the Homebrew PR. `brew upgrade linkup` now serves the new version, and
   installed binaries pick it up via `linkup update`.

## Publishing a beta

Betas are manual prerelease tags on `main` (or any commit). There's no rolling
auto-beta workflow.

1. From a clean `main` checkout, tag and push:

   ```sh
   git tag 4.1.0-rc.1
   git push origin 4.1.0-rc.1
   ```

   Use any SemVer-valid prerelease suffix (`-rc.N`, `-beta.N`, `-alpha.N`).

2. The tag triggers `release.yml` (dist), which builds artifacts and marks the
   GitHub release as a prerelease. The Homebrew publish step is skipped
   automatically for prereleases, so stable users on Homebrew are unaffected.
   The binary will report the tag version (not `Cargo.toml`'s) thanks to the
   `GITHUB_REF_NAME` override in `build.rs`.
3. Users on the beta channel pick it up via `linkup update --channel beta`. For
   fresh installs, use the installer from that specific release, for example:

   ```sh
   curl --proto '=https' --tlsv1.2 -LsSf https://github.com/mentimeter/linkup/releases/download/4.1.0-rc.1/linkup-cli-installer.sh | sh
   ```

`cleanup-prereleases.yml` runs weekly and keeps the latest 3 prereleases,
deleting older ones (release + tag).

## Patching an older version (hotfix)

Use this when `main` has accumulated unreleased work (the release-please PR is
open but not yet merged) and you need to ship a fix on the current stable line
without dragging that pending work into stable.

Currenty we do not support patching older major versions. Once a new major is
out, the previous one is EOL and won't receive updates.

There are no persistent support branches. Create one on demand and delete it
once the patch is out.

1. Branch off the tag you're patching:

   ```sh
   git switch -c hotfix/4.0.1 4.0.0
   git push -u origin hotfix/4.0.1
   ```

2. Open a PR **targeting `hotfix/4.0.1`** with the fix. Use a `fix:` PR title.
3. Merge it. release-please opens a "chore: release 4.0.1" PR on the same branch
   (it watches `hotfix/*` branches).
4. Merge the release PR, which tags `4.0.1`, dist builds artifacts, and opens a
   Homebrew PR.
5. **Forward-port the fix to `main`** by cherry-picking onto a new PR:

   ```sh
   git switch main && git pull
   git switch -c forward-port-fix-from-4.0.1
   git cherry-pick <sha-from-hotfix/4.0.1>
   ```

   Without this, the next release would regress the fix.

6. Delete the hotfix branch once the patch and forward-port are merged:

   ```sh
   git push origin --delete hotfix/4.0.1
   ```

## Modifying the dist (build/release) config

The build matrix, installers, and targets are configured in root `Cargo.toml`
under `[workspace.metadata.dist]`. If you change that section, regenerate the CI
workflow so it stays in sync:

```sh
cargo install cargo-dist     # one-time
dist generate                # rewrites .github/workflows/release.yml
```

Don't hand-edit `release.yml`. It's fully generated.
