#!/bin/bash
# Install MyPdfCraft on a Mac:
#   curl -fsSL https://raw.githubusercontent.com/hermi72/pdfcraft/main/scripts/install-mypdfcraft.sh | bash
#
# Installs the Apple command line tools and Rust when missing, downloads the fork to
# ~/git/pdfcraft and the translations to ~/git/craft-i18n, builds the app and puts it in
# ~/Applications/MyPdfCraft.app. Afterwards Help ▸ Check for updates… ▸ Update MyPdfCraft works
# (scripts/update-mypdfcraft.command). Safe to run again: it reuses what is already there.
set -uo pipefail

FORK="https://github.com/hermi72/pdfcraft"
UPSTREAM="https://github.com/storytold/pdfcraft"
REPO="$HOME/git/pdfcraft"
I18N="$HOME/git/craft-i18n"
APP="$HOME/Applications/MyPdfCraft.app"

step() { printf '\n\033[1m==> %s\033[0m\n' "$1"; }
fail() { printf '\n\033[31mStopped: %s\033[0m\n' "$1"; exit 1; }

[ "$(uname)" = Darwin ] || fail "this installer is for macOS."

step "Checking the Apple command line tools"
if ! xcode-select -p >/dev/null 2>&1; then
  xcode-select --install 2>/dev/null
  fail "macOS is now installing the command line tools (a window opened). When it is done, run this command again."
fi

step "Checking Rust"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
if ! command -v cargo >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y -q || fail "installing Rust failed."
  . "$HOME/.cargo/env"
fi

step "Downloading MyPdfCraft"
mkdir -p "$HOME/git"
if [ ! -d "$REPO/.git" ]; then
  git clone -q -o fork "$FORK" "$REPO" || fail "could not download $FORK."
fi
cd "$REPO" || fail "$REPO is missing."
git remote get-url origin >/dev/null 2>&1 || git remote add origin "$UPSTREAM"
git remote get-url fork >/dev/null 2>&1 || git remote add fork "$FORK"
git fetch -q fork && git checkout -q main && git merge -q --ff-only fork/main || fail "could not bring $REPO up to date."

step "Downloading the translations"
[ -d "$I18N/.git" ] || git clone -q https://github.com/storytold/craft-i18n "$I18N" || fail "could not download the translations."
git -C "$I18N" fetch -q && git -C "$I18N" checkout -q "$(cat craft-i18n.rev)" || fail "could not get the translations for this version."

step "Building MyPdfCraft (10-20 minutes the first time)"
SHA="$(git rev-parse --short HEAD)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
CRAFT_I18N_DIR="$I18N" CRAFT_I18N_REQUIRED=1 PDFCRAFT_BUILD_SHA="$SHA" cargo install -q --path apps/pdfcraft --locked \
  || fail "the build failed."

step "Making the app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$HOME/.cargo/bin/pdfcraft" "$APP/Contents/MacOS/PdfCraft"
cp assets/app-icon/pdfcraft.icns "$APP/Contents/Resources/PdfCraft.icns"
cp NOTICE packaging/macos/Credits.rtf "$APP/Contents/Resources/"
# Text recognition (OCR) models, when the official PdfCraft is installed to take them from.
if [ ! -d "$APP/Contents/Resources/models" ] && [ -d /Applications/PdfCraft.app/Contents/Resources/models ]; then
  cp -R /Applications/PdfCraft.app/Contents/Resources/models "$APP/Contents/Resources/models"
fi
sed -e "s/@VERSION@/$VERSION-claude/g" -e "s/@SHORT_VERSION@/$VERSION/g" -e "s/@BUILD_SHA@/$SHA/g" \
    -e "s#<string>ai.storyteller.pdfcraft</string>#<string>ai.storyteller.pdfcraft.claude</string>#" \
    -e "/CFBundleDisplayName/{n;s#PdfCraft#MyPdfCraft#;}" -e "/<key>CFBundleName/{n;s#PdfCraft#MyPdfCraft#;}" \
    packaging/macos/Info.plist.in >"$APP/Contents/Info.plist"
codesign --force --deep --sign - "$APP" 2>/dev/null || fail "signing the app failed."
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$APP" 2>/dev/null

step "Done: MyPdfCraft $VERSION is in $APP"
echo "Drag it from Applications (in your home folder) into the Dock."
echo "For the Claude buttons, install Claude Code and run 'claude' once to log in."
open "$APP"
