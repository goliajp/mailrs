#!/usr/bin/env bash
# ios-testflight.sh — the iPhone/iPad build, on TestFlight.
#
# The sibling of `mac-testflight.sh`, and the same shape: archive,
# sign for distribution, upload through Xcode's signed-in account. One
# app record carries all three platforms (universal purchase), so this
# and the Mac script put builds beside each other rather than into two
# separate apps.
#
# Usage:
#   ./scripts/ios-testflight.sh <version>            # build number 1
#   BUILD=3 ./scripts/ios-testflight.sh <version>    # a later upload
#
# App Store Connect refuses a build number it has already seen, and it
# says so **after** the upload rather than before it — so the number
# rises for every upload of the same version.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="${1:?usage: ios-testflight.sh <version>}"
TEAM=KF79DRC524
OUT="/tmp/mailrs-ios-release"
ARCHIVE="$OUT/Mailrs.xcarchive"

say() { printf '==> %s\n' "$*"; }
die() { printf '!! %s\n' "$*" >&2; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT"

say "[1/3] xcodegen"
(cd ios && xcodegen generate --spec project.yml >/dev/null)

say "[2/3] archive $VERSION (${BUILD:-1})"
(cd ios && xcodebuild -project Mailrs.xcodeproj -scheme Mailrs \
    -configuration Release -destination "generic/platform=iOS" \
    -archivePath "$ARCHIVE" \
    MARKETING_VERSION="$VERSION" CURRENT_PROJECT_VERSION="${BUILD:-1}" \
    -allowProvisioningUpdates archive) > "$OUT/archive.log" 2>&1 \
    || { grep -E "error:" "$OUT/archive.log" | sort -u | head; die "archive failed: $OUT/archive.log"; }

say "[3/4] export, and check what is in it"
# **Exported first, uploaded second.** `destination: upload` hands the
# build straight to Apple and leaves no file behind, so there is
# nothing to inspect — and the one thing worth inspecting is invisible
# afterwards: an app signed without production `aps-environment`
# installs, runs, and silently receives no push. The archive cannot be
# checked either; it is development-signed and the export is what
# re-signs for distribution. So: export to a file, look at the file,
# then send it.
cat > "$OUT/export.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key><string>app-store-connect</string>
    <key>teamID</key><string>$TEAM</string>
    <key>destination</key><string>export</string>
    <key>uploadSymbols</key><true/>
</dict>
</plist>
PLIST
(cd ios && xcodebuild -exportArchive -archivePath "$ARCHIVE" \
    -exportPath "$OUT/ipa" -exportOptionsPlist "$OUT/export.plist" \
    -allowProvisioningUpdates) > "$OUT/export.log" 2>&1 \
    || { grep -iE "error" "$OUT/export.log" | sort -u | head -5; die "export failed: $OUT/export.log"; }

IPA=$(ls "$OUT/ipa"/*.ipa 2>/dev/null | head -1)
[ -n "$IPA" ] || die "the export produced no ipa"
WORK=$(mktemp -d)
(cd "$WORK" && unzip -q -o "$IPA" 'Payload/Mailrs.app/*')
codesign -d --entitlements - "$WORK/Payload/Mailrs.app" 2>/dev/null \
    | grep -A2 "aps-environment" | grep -q "production" \
    || { rm -rf "$WORK"; die "no production aps-environment — push would never arrive"; }
# `2>&1` matters: codesign writes this to **stderr**, and grepping
# stdout finds nothing and reports a correctly-signed build as unsigned
# — a gate that fails a good release, which is the kind people delete.
SIGNED=$(codesign -dvv "$WORK/Payload/Mailrs.app" 2>&1 | grep -E "^Authority=" | head -1)
case "$SIGNED" in
    *"Apple Distribution"*) : ;;
    *) rm -rf "$WORK"; die "not signed for distribution — it is: ${SIGNED:-<codesign said nothing>}" ;;
esac
rm -rf "$WORK"
say "checked: Apple Distribution, production aps-environment"

say "[4/4] upload"
# The send is a second export of the same archive, with `destination`
# flipped to upload. There is no API key here: the account Xcode is
# signed into is what uploads, and that is the only path that uses it.
sed -i "" "s|<string>export</string>|<string>upload</string>|" "$OUT/export.plist"
(cd ios && xcodebuild -exportArchive -archivePath "$ARCHIVE" \
    -exportPath "$OUT/upload" -exportOptionsPlist "$OUT/export.plist" \
    -allowProvisioningUpdates) > "$OUT/upload.log" 2>&1 \
    || {
        grep -iE "error|missingApp|Invalid" "$OUT/upload.log" | sort -u | head -5
        LOGS=$(ls -dt /var/folders/*/*/T/Mailrs*.xcdistributionlogs 2>/dev/null | head -1)
        [ -n "$LOGS" ] && grep -ihE "step failed|error" "$LOGS"/*.log 2>/dev/null | tail -5
        die "upload failed: $OUT/upload.log"
    }
grep -q "Upload succeeded" "$OUT/upload.log" || die "no upload confirmation in $OUT/upload.log"

say "uploaded — App Store Connect is processing it"
grep -E "Upload succeeded|Uploaded" "$OUT/upload.log" | tail -2
