#!/usr/bin/env bash
# repro-gh-pages-push.sh — regression check for the Release workflow's manifest-
# publish git logic (.github/workflows/release.yml, "Generate and publish update
# manifest" step).
#
# It reproduces, in an ISOLATED throwaway environment, the bug that broke every
# first Release (exit 128) and proves the fix works:
#
#   Root cause: when `git clone --branch gh-pages` fails because the gh-pages
#   branch does not exist yet, the fallback did `git init` + `git checkout
#   --orphan gh-pages` but NEVER added an `origin` remote. The subsequent
#   `git push origin gh-pages` then died with:
#       fatal: 'origin' does not appear to be a git repository   (exit 128)
#
# Usage: scripts/repro-gh-pages-push.sh
# Exits 0 if all three scenarios behave as expected, 1 otherwise.
# Touches nothing outside a mktemp dir.

set -uo pipefail

REMOTE_URL_AUTH='https://x-access-token:FAKE@github.com/owner/repo.git'  # cosmetic
PASS=0
FAIL=0

ok()   { echo "  PASS: $1"; PASS=$((PASS+1)); }
bad()  { echo "  FAIL: $1"; FAIL=$((FAIL+1)); }

mk_remote() {
    # A bare repo with only a `master` branch (gh-pages absent) — mirrors the
    # real repo state that triggered the bug.
    local dir="$1"
    git init --bare -b master "$dir" >/dev/null 2>&1
}

# Emulates the OLD (buggy) publish logic from release.yml (pre-fix).
run_old_logic() {
    local remote="$1" pages="$2"
    local rc
    set +e
    (
        git clone --branch gh-pages --depth 1 "$remote" "$pages" 2>/dev/null || {
            git init "$pages" >/dev/null 2>&1
            cd "$pages"
            git checkout --orphan gh-pages >/dev/null 2>&1
            cd -
        }
        echo '{}' > "$pages/manifest.json"
        cd "$pages"
        git add manifest.json 2>/dev/null
        git -c user.email=a@b.c -c user.name=x commit -q -m m --allow-empty
        git push origin gh-pages >/dev/null 2>&1
    )
    rc=$?
    set -e
    return "$rc"
}

# Emulates the NEW (fixed) publish logic from release.yml (post-fix):
# distinguishes "branch missing" from a real clone error and adds `origin`
# in the orphan fallback before pushing.
run_new_logic() {
    local remote="$1" pages="$2"
    local rc
    set +e
    (
        local clone_err
        clone_err="$(mktemp)"
        if git clone --branch gh-pages --depth 1 "$remote" "$pages" 2>"$clone_err"; then
            rm -f "$clone_err"
        elif grep -q "Remote branch gh-pages not found" "$clone_err"; then
            rm -f "$clone_err"
            rm -rf "$pages"
            mkdir -p "$pages"
            git init "$pages" >/dev/null 2>&1
            cd "$pages"
            git checkout --orphan gh-pages >/dev/null 2>&1
            git remote add origin "$remote"        # <-- the fix
            cd -
        else
            cat "$clone_err" >&2; rm -f "$clone_err"; exit 99
        fi
        echo '{}' > "$pages/manifest.json"
        cd "$pages"
        git add manifest.json
        git -c user.email=a@b.c -c user.name=x commit -q -m m --allow-empty
        git push --force-with-lease origin gh-pages >/dev/null 2>&1
    )
    rc=$?
    set -e
    return "$rc"
}

set -e
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
REMOTE="$WORK/remote.git"
mk_remote "$REMOTE"

echo "Scenario 1 — OLD logic, first release (gh-pages absent): expect push to FAIL (the bug)"
if run_old_logic "$REMOTE" "$WORK/p1"; then
    bad "old logic unexpectedly succeeded"
else
    ok "old logic failed to push (exit != 0) — reproduces exit-128 class failure"
    # Confirm the failure reason: no origin remote in the orphan dir is the
    # exact cause; the bare remote still has no gh-pages.
    if git --git-dir="$REMOTE" branch --list gh-pages | grep -q gh-pages; then
        bad "gh-pages should NOT exist on remote after a failed old push"
    else
        ok "gh-pages absent from remote after failed old push"
    fi
fi

echo ""
echo "Scenario 2 — NEW logic, first release (gh-pages absent): expect push to SUCCEED (fix)"
if run_new_logic "$REMOTE" "$WORK/p2"; then
    ok "new logic pushed successfully on first release (orphan + origin + push)"
    if git --git-dir="$REMOTE" branch --list gh-pages | grep -q gh-pages; then
        ok "gh-pages branch created on remote"
    else
        bad "gh-pages missing on remote after new push"
    fi
else
    bad "new logic failed on first release (regression!)"
fi

echo ""
echo "Scenario 3 — NEW logic, second release (gh-pages now exists): expect clone path to SUCCEED"
if run_new_logic "$REMOTE" "$WORK/p3"; then
    ok "new logic pushed successfully on second release (clone path)"
else
    bad "new logic failed on second release (regression!)"
fi

echo ""
echo "----------------------------------------"
echo "PASS=$PASS  FAIL=$FAIL"
[ "$FAIL" -eq 0 ] && { echo "ALL GREEN ✓"; exit 0; } || { echo "FAILURES present ✗"; exit 1; }
