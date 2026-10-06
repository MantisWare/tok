#!/usr/bin/env bash
#
# Trigger the GitHub Actions release workflow for the version in Cargo.toml.
#
# Usage:
#   ./release.sh                    # release the current branch
#   ./release.sh --ref master       # release a specific branch or tag
#   ./release.sh --prerelease       # mark the GitHub release as a pre-release
#
# The workflow builds from whichever ref it is dispatched against and creates
# the tag there, so --ref decides what actually ships.

set -euo pipefail

REPO="MantisWare/tok"
REF="$(git rev-parse --abbrev-ref HEAD)"
PRERELEASE="false"

while [ $# -gt 0 ]; do
    case "$1" in
        --ref)
            REF="${2:?--ref needs a branch or tag}"
            shift 2
            ;;
        --prerelease)
            PRERELEASE="true"
            shift
            ;;
        -h | --help)
            sed -n '3,12p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "release.sh: unknown argument '$1' (try --help)" >&2
            exit 2
            ;;
    esac
done

if ! command -v gh > /dev/null 2>&1; then
    echo "release.sh: the GitHub CLI (gh) is not installed, so nothing was triggered." >&2
    echo "  Install it with:  brew install gh    (then: gh auth login)" >&2
    exit 127
fi

if ! gh auth status > /dev/null 2>&1; then
    echo "release.sh: gh is installed but not authenticated. Run: gh auth login" >&2
    exit 1
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [ -z "$VERSION" ]; then
    echo "release.sh: could not read the package version from Cargo.toml" >&2
    exit 1
fi
TAG="v$VERSION"

# A tag that already exists means this version shipped before; the workflow
# would move the release rather than create one.
if git ls-remote --exit-code --tags origin "$TAG" > /dev/null 2>&1; then
    echo "release.sh: $TAG already exists on origin. Bump the version in Cargo.toml first." >&2
    exit 1
fi

echo "Releasing $TAG from '$REF' (prerelease: $PRERELEASE)"

gh workflow run release.yml \
    --repo "$REPO" \
    --ref "$REF" \
    -f tag="$TAG" \
    -f prerelease="$PRERELEASE"

echo "Dispatched. Watch it with: gh run list --repo $REPO --workflow release.yml"
