# Releasing Tessera

Releases are a maintainer action. Normal contribution work ends with a PR.

## Prepare

1. Merge reviewed changes through the six required PR checks.
2. Set the same stable version in `apps/desktop/package.json`,
   `apps/desktop/src-tauri/Cargo.toml`, and
   `apps/desktop/src-tauri/tauri.conf.json`. Refresh the desktop Cargo lockfile
   when its package version changes. The private root workspace version is separate.
   The standalone desktop crate must keep an explicit literal `[package]` version;
   workspace-inherited Cargo versions are not supported by this release policy.
3. Update the changelog, run `pnpm guard:pre-push`, and merge the preparation PR.

## Tag and build

Choose an unused `vMAJOR.MINOR.PATCH` tag on reviewed master. Validate locally
first; for example, if the desktop version is 0.2.0:

```bash
node tools/scripts/check-release.mjs refs/tags/v0.2.0
```

This example validates a version; it is not an instruction to reuse a tag.

The [release workflow](../.github/workflows/release.yml) runs on `v*` tag pushes.
Manual reruns must select the tag ref. Preflight rejects branch refs, malformed
or prerelease tags, version mismatches, and commits outside master. Only stable
releases are supported.

Full reusable CI runs before Linux, macOS universal, and Windows bundling.
Advisory jobs retain their advisory behavior. Bundles create/update a **draft**
GitHub release. Diagnose failures and rerun; do not move a tag to different code
or bypass validation.

## Review the draft

- Confirm all platform assets use the expected tag.
- Install and launch each target app; smoke-test provider setup, import,
  generation, export, and supported opt-in sandbox behavior.
- Describe known limits and changed provider, persistence, or network boundaries.
- Verify signing/notarization before claiming them; this workflow configures neither.
- Publish manually after review.

A draft or green bundle job does not establish public availability. Repair a
broken published release with a new patch release and clear notes.
