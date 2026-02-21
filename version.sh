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

# Cross-platform sed -i wrapper (macOS vs Linux)
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

if [[ -n $(git status --porcelain) ]]; then
  echo -e "${CLR_WARN}[WARN] You have uncommitted changes.${CLR_RESET}"
  read -p "Continue anyway? (y/N): " confirm && [[ $confirm == [yY] ]] || exit 1
fi

# Extract version specifically from the [package] section
current=$(grep -m 1 '^version = ' "$CARGO_TOML" | cut -d '"' -f 2)

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

# Update Cargo.toml (only the first match)
inplace_edit "0,/version = \"$current\"/s//version = \"$new\"/" "$CARGO_TOML"

# Update tauri.conf.json
inplace_edit "s/\"version\": \"$current\"/\"version\": \"$new\"/" "$TAURI_CONF"

# Sync lockfile
cargo generate-lockfile --manifest-path "$CARGO_TOML" 2>/dev/null

echo -e "${CLR_SUCCESS}[OK] Files updated to $new${CLR_RESET}"

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
  echo -e "${CLR_WARN}[ABORT] Files remain updated but uncommitted.${CLR_RESET}"
fi