#!/bin/bash

# This script is used to release a new version of the project.
# It is used to create a new tag and push it to the remote repository.
# It is also used to create a new release on GitHub.

# Get the current version from the Cargo.toml file
CURRENT_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')

# Increment the patch version
PATCH_VERSION=$((CURRENT_VERSION + 1))

# Create a new tag with the incremented patch version
git tag -a "v$PATCH_VERSION" -m "Release v$PATCH_VERSION"

# Push the tag to the remote repository
git push origin "v$PATCH_VERSION"

# Create a new release on GitHub
gh release create "v$PATCH_VERSION" --title "Release v$PATCH_VERSION" --notes "Release v$PATCH_VERSION"
