# Publication authenticates through Trusted Publishing, never a stored token

The release line's publish job holds no registry credential. The crate publishes
through `rust-lang/crates-io-auth-action@v1`, which exchanges the runner's OpenID
Connect identity for a token that lives for the length of one `cargo publish`; the
Python distribution publishes through `pypa/gh-action-pypi-publish`, which presents
the same identity directly. The alternative — a long-lived API token in repository
secrets — was rejected without much of a fight: a credential that outlives every
review this line runs is the one asset in the project that cannot be re-derived, and
both registries now mint short-lived credentials precisely so nobody has to keep one.

The claim the registries match is exact: repository owner and name, the workflow file
name (`release.yml`), and the GitHub environment (`release`). Three strings must
agree across three systems, so the environment name is written once in the workflow
and held to `docs/releasing.md` by `cargo xtask check-workflow` — renaming it is a
three-sided change, and a half-done rename fails closed (403), which is the correct
failure for a publication identity that no longer describes itself.

Two consequences of the registries' own rules shaped the line:

- **crates.io requires the crate to exist first.** Trusted Publishing cannot be bound
  to a crate that has never been published, so v0.1.0's crate half is published by
  hand from the tag with a one-time, short-expiry `publish-new` token that is revoked
  immediately after (the runbook owns the steps). The pipeline compensates: before
  publishing, it asks `index.crates.io` whether the number is already there and skips
  loudly when it is — otherwise following the runbook would die on the registry's own
  "already exists" refusal and drag the Python half down with it.
- **The publication steps live in `release.yml` itself.** A reusable workflow cannot
  be a Trusted Publishing publisher at all — the OIDC claim names a workflow file, and
  delegation is invisible to it. `check-workflow` refuses any local `uses:` so the
  prohibition is held by a check and not by a memory.

## Consequences

- No token ever sits in the repository, in CI variables, or in this document. The one
  manual publish's token exists for minutes and is revoked on a runbook step of its
  own.
- A leaked maintainer account is not enough to publish: the job's identity, not a
  person's, is what the registries accept.
- The exact-match binding makes the environment name part of the release contract.
  Moving the publish job to another workflow file, or renaming the environment, is a
  coordinated edit across GitHub, PyPI, and crates.io.
