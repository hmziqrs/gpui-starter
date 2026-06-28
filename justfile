default:
    @just --list

mac *args='':
  ./scripts/run-macos-dev-app.sh {{args}}

fmt:
    cargo sort-derives
    cargo fmt
    taplo fmt
    rumdl fmt .

clippy:
    cargo clippy --workspace --all-features --exclude some-lib-forms

check:
    cargo check --workspace --all-features --exclude some-lib-forms

test:
    cargo test --workspace --all-features

test-publish:
    cargo publish --workspace --dry-run --allow-dirty

# ---------------------------------------------------------------------------
# Release operations.
#
# CI runs on the FORK hmziqagent/gpui-starter (remote `fork`), tag-triggered:
# pushing a v* tag fires .github/workflows/release.yml from the tagged commit.
# publish-release uses softprops/action-gh-release@v2, which UPSERTS the GitHub
# Release (replaces assets, regenerates notes) — so a re-trigger updates the
# already-published release in place; no manual release deletion required.
# ---------------------------------------------------------------------------
release-repo    := "hmziqagent/gpui-starter"
release-remote  := "fork"
release-workflow := "release.yml"
# Latest vX.Y.Z tag reachable from HEAD, as a bare version (no leading 'v').
latest-version  := `git describe --tags --abbrev=0 2>/dev/null | sed 's/^v//'`

# Force re-release a version (default: latest tag) built from the current HEAD.
#
# Moves the tag to HEAD, then deletes + re-pushes it to the fork — the reliable
# way to re-fire a tag-push Release event (a bare force-push is not always
# delivered by GitHub). The published release for that version is updated in
# place by action-gh-release.
#
# A tag marks a COMMIT, not your working tree — commit what you want shipped
# BEFORE running this, otherwise it just re-builds the last commit unchanged.
force-release version=latest-version:
    #!/usr/bin/env bash
    set -euo pipefail
    v="$(./scripts/normalize-version.sh "{{version}}")"
    tag="v${v}"
    old="$(git rev-list -n 1 "${tag}" 2>/dev/null || true)"
    echo "==> Force re-release ${tag} on {{release-repo}}"
    echo "    HEAD : $(git rev-parse --short HEAD)  $(git log -1 --format=%s HEAD)"
    if [ -n "${old}" ] && [ "${old}" != "$(git rev-parse HEAD)" ]; then
        echo "    was : $(git rev-parse --short "${old}")  (moving tag -> HEAD)"
        git log --oneline "${old}..HEAD" | sed 's/^/      /'
    fi
    # Point the tag at HEAD (local), then delete + recreate it on the remote so
    # GitHub delivers a fresh tag-push event.
    git tag -f "${tag}" HEAD
    git push {{release-remote}} ":refs/tags/${tag}" 2>/dev/null || true
    git push {{release-remote}} "refs/tags/${tag}"
    echo "==> Triggered. Watch: just watch-release"

# Fire the Release workflow via workflow_dispatch on a ref (default
# feat/linux-infra) for a version, WITHOUT moving any tag. The release is
# UPSERTED. Use this to rebuild from a branch tip when you don't want to
# mutate tags.
dispatch-release version=latest-version ref='feat/linux-infra':
    #!/usr/bin/env bash
    set -euo pipefail
    v="$(./scripts/normalize-version.sh "{{version}}")"
    echo "==> Dispatch {{release-workflow}} (version=${v}, ref={{ref}}) on {{release-repo}}"
    gh workflow run {{release-workflow}} -R {{release-repo}} --ref "{{ref}}" -f version="${v}"
    echo "==> Triggered. Watch: just watch-release"

# Watch the most recent Release run (or a specific run id) to completion.
# Exits non-zero if the run fails.
watch-release runid='':
    #!/usr/bin/env bash
    set -euo pipefail
    runid="{{runid}}"
    if [ -z "${runid}" ]; then
      runid="$(gh run list -R {{release-repo}} --workflow={{release-workflow}} --limit 1 --json databaseId --jq '.[0].databaseId')"
    fi
    echo "==> Watching Release run ${runid} on {{release-repo}}"
    gh run watch "${runid}" -R {{release-repo}} --exit-status

# List recent Release runs on the fork.
release-runs limit='8':
    gh run list -R {{release-repo}} --workflow={{release-workflow}} --limit {{limit}}
