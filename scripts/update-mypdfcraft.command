#!/bin/bash
# Update MyPdfCraft: bring in the official PdfCraft changes and the fork's own, test, upload to the
# fork, rebuild ~/Applications/MyPdfCraft.app and restart it. Started by Help ▸ Check for
# updates… ▸ Update MyPdfCraft (it opens this file in Terminal), or by hand.
#
# Stops without changing anything when the project folder has unsaved changes, when a community
# change conflicts with ours, or when the tests fail. Remotes: origin = storytold/pdfcraft,
# fork = hermi72/pdfcraft.
set -uo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
APP="${MYPDFCRAFT_APP:-$HOME/Applications/MyPdfCraft.app}"
BUNDLE_ID="ai.storyteller.pdfcraft.claude"

step() { printf '\n\033[1m==> %s\033[0m\n' "$1"; }
finish() { printf '\n'; read -r -p "Press Return to close this window. " _; exit "$1"; }
fail() {
  printf '\n\033[31mStopped: %s\033[0m\nMyPdfCraft was not changed.\n' "$1"
  finish 1
}
running() { pgrep -f "$APP/Contents/MacOS/" >/dev/null; }

cd "$REPO" || fail "the project folder $REPO is missing."
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
command -v cargo >/dev/null || fail "Rust (cargo) is not installed on this Mac."
[ -d "$APP" ] || fail "$APP is missing."
[ "$(git branch --show-current)" = main ] || fail "the project folder is not on the main branch. Ask Claude to sort it out."
if ! git diff --quiet || ! git diff --cached --quiet; then
  fail "the project folder has changes that are not committed. Commit them first, or ask Claude."
fi

step "Downloading changes from GitHub"
git fetch -q origin && git fetch -q fork || fail "could not reach GitHub."
START="$(git rev-parse HEAD)"
undo() { git merge --abort 2>/dev/null; git reset -q --hard "$START"; }
git merge -q --ff-only fork/main || fail "your fork and this Mac have different changes. Ask Claude to bring them together."

COMMUNITY="$(git rev-list --count HEAD..origin/main)"
if [ "$COMMUNITY" -gt 0 ]; then
  step "Bringing in $COMMUNITY community changes"
  if ! git merge -q --no-edit origin/main; then
    undo
    fail "a community change conflicts with your changes. Ask Claude: \"update my PdfCraft, there is a merge conflict\"."
  fi
else
  echo "No new community changes."
fi

step "Testing (a few minutes)"
if ! cargo test -q -p pdfcraft-ui-egui -j 4 --lib -- claude my_updates i18n command; then
  undo
  fail "the tests failed after the update. Ask Claude to look at it."
fi

if [ "$(git rev-parse HEAD)" != "$(git rev-parse fork/main)" ]; then
  step "Uploading to your fork"
  # A Mac without a GitHub login can't upload; the app is still updated.
  git push -q fork main || echo "Uploading to GitHub didn't work (not logged in?). Continuing; this Mac is updated anyway."
fi

step "Building MyPdfCraft (5-10 minutes)"
SHA="$(git rev-parse --short HEAD)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
# The translations (German and the others) come from craft-i18n at the commit in craft-i18n.rev;
# without them the interface is English only.
I18N="${CRAFT_I18N_DIR:-$HOME/git/craft-i18n}"
[ -d "$I18N/.git" ] || git clone -q https://github.com/storytold/craft-i18n "$I18N" || fail "could not download the translations."
git -C "$I18N" fetch -q && git -C "$I18N" checkout -q "$(cat craft-i18n.rev)" || fail "could not get the translations for this version."
CRAFT_I18N_DIR="$I18N" CRAFT_I18N_REQUIRED=1 PDFCRAFT_BUILD_SHA="$SHA" cargo install -q --path apps/pdfcraft --locked -j 4 || fail "the build failed. Ask Claude to look at it."

step "Installing"
if running; then
  echo "Closing MyPdfCraft (save your work if it asks)..."
  osascript -e "tell application id \"$BUNDLE_ID\" to quit" >/dev/null 2>&1
  for _ in $(seq 1 120); do running || break; sleep 1; done
  running && fail "MyPdfCraft is still open. Quit it, then run the update again (the build is kept, so it is quick)."
fi
cp "$HOME/.cargo/bin/pdfcraft" "$APP/Contents/MacOS/.PdfCraft.new" && mv -f "$APP/Contents/MacOS/.PdfCraft.new" "$APP/Contents/MacOS/PdfCraft" \
  || fail "could not copy the new version into $APP."
PLIST="$APP/Contents/Info.plist"
plutil -replace CFBundleShortVersionString -string "$VERSION" "$PLIST"
plutil -replace CFBundleVersion -string "$VERSION" "$PLIST"
plutil -replace PdfCraftVersion -string "$VERSION-claude" "$PLIST"
plutil -replace PdfCraftBuildCommit -string "$SHA" "$PLIST"
codesign --force --deep --sign - "$APP" 2>/dev/null || fail "signing the app failed."

step "Done: MyPdfCraft $VERSION ($SHA)"
open "$APP"
finish 0
