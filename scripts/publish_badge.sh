#!/usr/bin/env bash
# Publish a shields.io endpoint badge to the `badges` branch, which the
# README reads through raw.githubusercontent.com. That branch holds badge
# JSON and nothing else, so it's force-pushed as a single commit rather than
# accumulating history. Unchanged badges aren't pushed at all.
#
# Usage: GITHUB_TOKEN=... scripts/publish_badge.sh badge/coverage.json
set -euo pipefail

badge=${1:?usage: publish_badge.sh FILE}
: "${GITHUB_TOKEN:?publish_badge: GITHUB_TOKEN is required}"
repo=${GITHUB_REPOSITORY:?publish_badge: GITHUB_REPOSITORY is required}
name=$(basename "$badge")
url="https://raw.githubusercontent.com/$repo/badges/$name"
# Overridable so the push can be exercised against a local repo.
remote=${BADGE_REMOTE:-"https://x-access-token:$GITHUB_TOKEN@github.com/$repo"}

if curl -sf "$url" | diff -q - "$badge" >/dev/null 2>&1; then
    echo "publish_badge: unchanged, nothing to push ($url)"
    exit 0
fi

work=$(mktemp -d)
cp "$badge" "$work/$name"
cd "$work"
git init -q -b badges
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git add "$name"
git commit -q -m "chore: badges for ${GITHUB_SHA:-a local run}"
git push -q --force "$remote" badges
echo "publish_badge: pushed $name to the badges branch ($url)"
