#!/usr/bin/env bash

set -e

# --- Configuration ---
CARGO_TOML="src-tauri/Cargo.toml"
TAURI_CONF="src-tauri/tauri.conf.json"

# --- Colors ---
CLR_RESET='\033[0m'
CLR_WARN='\033[1;33m'
CLR_ERROR='\033[1;31m'
CLR_SUCCESS='\033[1;32m'
CLR_INFO='\033[1;36m'
CLR_BOLD='\033[1m'

# --- Helper Functions ---

# Cross-platform sed -i wrapper
inplace_edit() {
  local pattern=$1
  local file=$2
  if [[ "$OSTYPE" == "darwin"* ]]; then
    sed -i '' "$pattern" "$file"
  else
    sed -i "$pattern" "$file"
  fi
}

# --- Pre-flight Checks ---

# STRICT MODE: Exit if there are any uncommitted changes
if [[ -n $(git status --porcelain) ]]; then
  echo -e "${CLR_ERROR}[ERROR] Working directory is not clean.${CLR_RESET}"
  echo -e "Please commit or stash your changes before bumping the version."
  exit 1
fi

# Extract version specifically from the [package] section
# Handles both 'version = "1.0.0"' and 'version="1.0.0"'
current=$(grep -m 1 '^version' "$CARGO_TOML" | cut -d '"' -f 2)

if [[ -z "$current" ]]; then
  echo -e "${CLR_ERROR}[ERROR] Could not find version in $CARGO_TOML${CLR_RESET}"
  exit 1
fi

IFS='.' read -r major minor patch <<< "$current"

# --- Version Logic ---

case "${1:-}" in
  major) major=$((major + 1)); minor=0; patch=0 ;;
  minor) minor=$((minor + 1)); patch=0 ;;
  patch) patch=$((patch + 1)) ;;
  *)
    echo -e "${CLR_BOLD}Usage:${CLR_RESET} $0 [major|minor|patch]"
    echo -e "${CLR_INFO}Current version:${CLR_RESET} $current"
    exit 1
    ;;
esac

new="$major.$minor.$patch"
echo -e "${CLR_INFO}[BUMP]${CLR_RESET} $current -> ${CLR_BOLD}$new${CLR_RESET}"

# --- Execution ---

# Update Cargo.toml (Flexible regex for spaces around '=')
inplace_edit "s/^version[[:space:]]*=[[:space:]]*\"$current\"/version = \"$new\"/" "$CARGO_TOML"

# Update tauri.conf.json
inplace_edit "s/\"version\":[[:space:]]*\"$current\"/\"version\": \"$new\"/" "$TAURI_CONF"

# Force the lockfile to sync the new version
# We use the package name from Cargo.toml to perform a targeted update
PKG_NAME=$(grep -m 1 '^name' "$CARGO_TOML" | cut -d '"' -f 2)
cargo update --manifest-path "$CARGO_TOML" -p "$PKG_NAME" --offline 2>/dev/null

echo -e "${CLR_SUCCESS}[OK] Files and Lockfile updated to $new${CLR_RESET}"

# --- Git Workflow ---

echo -en "${CLR_BOLD}Ready to commit and push v$new? (y/N): ${CLR_RESET}"
read -r confirm
if [[ $confirm == [yY] ]]; then
  git add "$CARGO_TOML" "$TAURI_CONF" src-tauri/Cargo.lock
  git commit -m "chore: bump version to $new"
  git tag -a "v$new" -m "Release v$new"
  
  echo -e "${CLR_INFO}[GIT] Pushing to origin...${CLR_RESET}"
  git push origin main --follow-tags
  echo -e "${CLR_SUCCESS}[DONE] Pushed v$new${CLR_RESET}"
else
  echo -e "${CLR_WARN}[ABORT] Version files updated but not committed.${CLR_RESET}"
  echo -e "You will need to manually commit or revert changes."
fi