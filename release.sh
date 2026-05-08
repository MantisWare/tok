#!/bin/bash

# This script triggers the GitHub Actions release workflow for this project
# Usage: ./release.sh

# Get the current version from the Cargo.toml file
TAG=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')

echo "Triggering GitHub Actions release.yml workflow for version $TAG..."

gh workflow run release.yml --repo mantisware/tok -f tag="v$TAG"

echo "Release workflow triggered for tag v$TAG!"
