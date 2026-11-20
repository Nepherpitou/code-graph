# Releases

Tagging a commit on `master` publishes prebuilt binaries:

```bash
git tag v0.1.0      # must match the version in Cargo.toml
git push origin v0.1.0
```

`.github/workflows/release.yml` builds four targets: `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl` (static, cross-built with zig), `aarch64-apple-darwin`,
and `x86_64-apple-darwin`. Assets are `code-graph-<version>-<target>`, each with its
own `.sha256`, plus one `SHA256SUMS`:

```bash
gh release download v0.1.0
sha256sum -c SHA256SUMS --ignore-missing
chmod +x code-graph-0.1.0-x86_64-unknown-linux-musl
```

The tag is rejected unless it points to a commit on `master` and equals `v` + the version
from `Cargo.toml`; the publish job runs only after all four builds succeed.

## Why zig

The Linux binaries are static musl builds, and the bundled SQLite needs a C compiler that
cc-rs can name. Debian's `musl-tools` ships only `musl-gcc`, not the
`x86_64-linux-musl-gcc` cc-rs looks for; `cargo-zigbuild` supplies a cross `cc` and linker
for both Linux architectures and removes the per-target toolchain setup.

## The vendored binary

`vendor/code-graph` stays in the repository on purpose: it is the binary this repository's
own CI runs, so pull requests exercise the same artifact users get, built with
`docker.io/library/rust:1-alpine` and `--profile dist` for `x86_64-unknown-linux-musl`.
Releases are the channel for external consumers; the vendor copy is the one pinned by
commit in downstream CI.
