# The root workspace manifest's version is the Authoritative Version

One number decides a Release: the `version` in `[workspace.package]` of the root
`Cargo.toml`. Everything else in the tree that states a version is a derived copy —
the binding crate's manifest, and whatever the Python metadata ends up carrying — and
`cargo xtask sync-version` is the only thing allowed to write a copy, one way, from
the root. A tag is checked against that number, not against a copy.

The Python side holds no static version at all. `pyproject.toml` declares
`dynamic = ["version"]` and lets maturin read the number from the binding crate's
manifest, which `sync-version` derived; `__init__.py` takes `__version__` from the
compiled `_core` at runtime, where it was frozen by `env!("CARGO_PKG_VERSION")`. The
alternative — a version field in `pyproject.toml` kept in step by the same sync — was
rejected because a copy that a check must keep equal to another copy is a second
opinion, not a source: any day the check is skipped (or a copy is edited by hand to
unblock a build), the wheel and the crate can disagree about what shipped, and a
Registry turns that disagreement permanent. With `dynamic`, the metadata cannot
disagree because it has no opinion of its own.

The release path never rewrites repository content. A workflow that bumps versions,
edits the changelog, or rewrites manifests on a tag was rejected on purpose: a run
that commits while it publishes mixes two authorities in one act, needs write
credentials in CI that publication does not otherwise require, and turns "what does
main say" into a race between a person and a machine. The number is decided before
the tag exists; the tag push is the decision, not a request for one. The only
post-release write is a human one — restoring `[Unreleased]` at the top of the
changelog — because the changelog records what happened, and a record written by the
event it records is a log, not a decision.

## Consequences

- Version drift is caught at pull-request time (`sync-version --check` in `ci.yml`),
  not at release time, and the check names the files that moved.
- The changelog's dated section is part of what makes a tag releasable
  (`check-release` refuses an undated number), so the release notes can be composed
  from the changelog instead of being typed on release day.
- Bumping the version is a one-line edit to the root manifest plus a commit; every
  copy re-derives or the gate goes red.
