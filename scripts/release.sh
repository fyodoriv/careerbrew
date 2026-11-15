#!/usr/bin/env bash
# Cut a careerbrew release in two steps.
#
#   scripts/release.sh <x.y.z>   on a branch: set the version in pyproject.toml and uv.lock
#                                (`uv version`), add a CHANGELOG.md entry from the merged pull
#                                request titles since the last tag, and commit. Then open a PR.
#   scripts/release.sh tag       after that PR merged: tag main's tip `v<version>` and push the tag.
#
# The version is a plain x.y.z (telemetry sends only that). It must be higher than the current one.
set -euo pipefail
cd "$(dirname "$0")/.."

usage() { echo "usage: scripts/release.sh <x.y.z> | tag" >&2; exit 2; }
current() { sed -nE 's/^version = "([^"]+)"$/\1/p' "${1:-pyproject.toml}" | head -1; }

arg="${1:-}"
if [ "$arg" = tag ]; then
  git fetch -q origin main --tags
  version="$(git show origin/main:pyproject.toml | sed -nE 's/^version = "([^"]+)"$/\1/p' | head -1)"
  [ -n "$version" ] || { echo "release: no version in origin/main:pyproject.toml" >&2; exit 1; }
  if git rev-parse -q --verify "refs/tags/v$version" >/dev/null; then
    echo "release: v$version is already tagged at $(git rev-parse --short "v$version^{commit}")"
    exit 0
  fi
  git tag -a "v$version" -m "careerbrew $version" origin/main
  git push origin "refs/tags/v$version"
  echo "release: tagged v$version at $(git rev-parse --short origin/main)"
  exit 0
fi

[[ "$arg" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || usage
now="$(current)"
if [ "$arg" = "$now" ] || [ "$(printf '%s\n%s\n' "$now" "$arg" | sort -V | tail -1)" != "$arg" ]; then
  echo "release: $arg must be higher than the current version $now" >&2
  exit 2
fi
branch="$(git rev-parse --abbrev-ref HEAD)"
[ "$branch" != main ] || { echo "release: run it on a branch, then merge it through a pull request" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "release: the working tree has changes; commit them first" >&2; exit 1; }

last="$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)"
range="${last:+$last..}HEAD"
uv version --quiet "$arg"

# Squash merges keep the pull request number in the subject: "feat: ... (#92)".
entries="$(git log --no-merges --format='- %s' "$range" | grep -E '\(#[0-9]+\)$' || true)"
[ -n "$entries" ] || entries="- No merged pull requests since ${last:-the first commit}."
{
  echo "# Changelog"
  echo
  echo "## $arg ($(date -u +%Y-%m-%d))"
  echo
  echo "$entries"
  if [ -f CHANGELOG.md ]; then
    tail -n +2 CHANGELOG.md
  fi
} > CHANGELOG.md.new
mv CHANGELOG.md.new CHANGELOG.md

git add pyproject.toml uv.lock CHANGELOG.md
git commit -q -m "chore: release $arg"
echo "release: $arg committed on $branch. Next: open a PR; after it merges, run scripts/release.sh tag"
