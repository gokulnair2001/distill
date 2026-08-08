# Releasing

Distribution is automated with [dist](https://github.com/axodotdev/cargo-dist).
On a version tag, GitHub Actions cross-compiles the binaries, publishes a GitHub
Release (prebuilt archives + shell installer), and publishes the npm package.

The published package name is **`distill-md`** (the names `distill` are taken on
crates.io and npm). The installed commands are still **`distill`** and
**`distill-mcp`** — both binaries ship in every artifact (the release builds with
`--features mcp`).

## One-time setup

1. **Secrets** (repo → Settings → Secrets and variables → Actions):
   | Secret | Needed for | How to get it |
   |---|---|---|
   | `GITHUB_TOKEN` | Creating the GitHub Release | Automatic — nothing to do |
   | `NPM_TOKEN` | Publishing `distill-md` to npm | An npm **Automation** access token (`npm token create`) |
2. **Claim the npm name** — `distill-md` must be free on npm (or switch to a
   scoped name like `@gokulnair2001/distill-md` in `dist-workspace.toml`).

> If you're not ready to publish to npm yet, remove `"npm"` from `installers` /
> `publish-jobs` in `dist-workspace.toml` and re-run `dist generate`. The GitHub
> Release with prebuilt binaries + shell installer needs no extra secrets.

## Cutting a release

```bash
# 1. Bump the version in Cargo.toml (e.g. 0.1.0 -> 0.1.1), commit.
# 2. Tag and push — the tag drives everything.
git tag v0.1.1
git push origin v0.1.1
```

The `release` workflow then builds all targets, creates the GitHub Release, and
runs the npm publish job.

### Dry-run locally

```bash
dist plan          # show exactly what a release would produce
dist build         # build this host's artifacts locally into target/distrib/
```

## Changing the pipeline

Edit `dist-workspace.toml`, then regenerate the workflow:

```bash
dist generate
```

Never hand-edit `.github/workflows/release.yml` — `dist generate` overwrites it.
