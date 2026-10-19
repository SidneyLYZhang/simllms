# Releasing

A Release is one act: the current code and one Catalog Snapshot freeze into a single
version number on both registries at once. Rust and Python never release apart, a
registry never re-prints a number, and the first number is published by a person, not
by a pipeline. This file is the runbook — execute a release from it, not from memory.

The whole line lives in `.github/workflows/release.yml`. Everything except the two
registry uploads and the release page is rehearsed by a dry run whenever you like:

```sh
gh workflow run release.yml --ref main -f dry_run=true
```

A rehearsal builds every artifact, inventories the set, and runs the seam suite in a
clean environment, then skips the publish job visibly. Run one before cutting a tag.

`cargo xtask check-workflow` holds this file to the workflow it serves: the approval
environment is named here and there, and the sections below are checked to exist in
the order a first Release meets them.

Environment name: `release`

The GitHub approval environment is named `release`, once, on the `publish` job in
`release.yml`. Both registry-side bindings below are keyed to that exact name —
renaming it is a three-sided change (GitHub, PyPI, crates.io), not a rename.

## Verify the registry account email

Both registries refuse uploads from an account whose email is not verified, and the
refusal surfaces only at publish time.

1. Log in to [crates.io](https://crates.io) and open Account Settings; the email must
   show as verified. The crate publishes under this account, and its owner becomes
   the crate's owner — this cannot be changed casually afterwards.
2. Log in to [PyPI](https://pypi.org) and confirm the account there is verified too.

## Create the approval environment

The `publish` job waits for a human reviewer on the `release` environment. A first
run creates the environment automatically *without* a reviewer, so create it first.

1. GitHub repository → Settings → Environments → New environment → name it exactly
   `release`.
2. Add a required reviewer (the person who approves publication). One is enough; the
   approval covers the crate, the Python distribution, and the GitHub Release page,
   because they are one job.
3. Do not add deployment branches or wait timers. The gate is the reviewer.

## Bind the pending Python publisher

PyPI's Trusted Publishing can be bound before the project exists, as a *pending
publisher*. The OIDC claim is matched exactly — repository, workflow file name,
environment — so every field must be spelled as the workflow spells it.

1. PyPI → Account Settings → Publishing → Add a new pending publisher.
2. PyPI project name: `simllms`; owner: `SidneyLYZhang`; repository: `simllms`;
   workflow: `release.yml`; environment: `release`.

## Publish the crate once with a short-lived token

Only for the crate's first version. crates.io's Trusted Publishing cannot be bound to
a crate that has never been published, so v0.1.0's crate half is published by hand,
from the tag, with a token that lives minutes.

1. Push nothing yet — this step runs when the pipeline is already waiting at the
   approval, after the artifacts are proven (see *Cut a release*, step 3).
2. crates.io → Account Settings → API Tokens → New token. Scope it as narrowly as the
   form allows and give it the shortest expiry available. Copy it once; it is shown
   once.
3. Check out the tag: `git fetch --tags && git checkout vX.Y.Z`.
4. Publish: `CARGO_REGISTRY_TOKEN=<token> cargo publish -p simllms`. This compiles
   the unpacked crate exactly as the pipeline's own publish would.
5. Confirm the crate renders on crates.io with its README and license files.

## Revoke the token

Immediately after the crate is visible on crates.io — before anything else.

1. crates.io → Account Settings → API Tokens → Revoke the token created above.
2. Confirm revocation: the token list no longer shows it. A token that was never
   stored anywhere cannot leak later; a token revoked after one use cannot leak now.

## Bind the crate's trusted publisher

Now that the crate exists, crates.io accepts its publisher binding, and every later
version publishes through the pipeline with no token at all.

1. crates.io → the `simllms` crate page → Settings → Trusted Publishing → add:
   owner `SidneyLYZhang`, repository `simllms`, workflow `release.yml`,
   environment `release`.

## Cut a release

From here on, every version — including v0.1.0's Python half and release page — is
published by the pipeline.

1. Rehearse: run the dry run on `main` (command at the top) and let it finish green.
   A red rehearsal is a stopped release; fix and re-rehearse.
2. `cargo xtask check-release "$(cargo xtask release-tag)"` on `main` must pass: the
   version is final, the tag's commit has landed on `main`, and the changelog holds a
   dated section for the number.
3. Tag and push: `git tag "$(cargo xtask release-tag)" && git push origin "$(cargo
   xtask release-tag)"`. The pipeline starts on its own. While it builds, this is the
   window for the first-release crate publish above.
4. The `publish` job stops at the `release` environment's reviewer. Review the run:
   gates green, five wheels, inventory green, seam green, the number is the one the
   changelog dates. Then approve.
5. Confirm: the version is visible on both crates.io and PyPI, and the GitHub Release
   page carries the changelog section, the platform list, and the Catalog Snapshot
   date and upstream commit.
6. On a clean machine, install from each registry and re-run the seam assertions:
   `uv pip install simllms==X.Y.Z` in a fresh venv with `SIMLLMS_RELEASE_TAG=vX.Y.Z`
   set, then run `crates/simllms-python/tests/test_release_artifacts.py`; and
   `cargo add simllms` (or download the `.crate`) to confirm the crate's side.

## When a release half-fails

The policy is **Forward Fix**: repair by cutting the next number, never by removing
what already shipped. Registries keep old numbers forever; a number that reached a
registry stays reachable. Fix the cause on `main`, date a new changelog section for
the next number, and cut again. A half failure is one registry holding the number —
the next Release completes the pair.

## Yank a release

A human step, done by hand in the registry's web UI; nothing in this repository or
its workflows yanks. Yanking removes the files from download but does not free the
number — it stays burned.

1. crates.io → the crate version → Yank. PyPI → the release → delete (PyPI has no
   yank; deletion removes the files and burns the number the same way).
2. Only for content that is broken in a way that harms its users — a wrong or
   vulnerable artifact — never to retry a failed upload.
3. Note it in the changelog under the version's section, and on the GitHub Release
   page, so the number's history says what happened to it.

## After a release

1. On `main`, put `## [Unreleased]` back at the top of `CHANGELOG.md`, above the
   section just released.
2. Run `cargo xtask sync-package` so the packages' carried copies re-derive, and
   commit both with the changelog.
3. The next number is chosen by what lands: new public API is a minor bump; fixes
   ride on a patch.
