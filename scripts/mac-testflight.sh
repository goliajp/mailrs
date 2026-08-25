#!/usr/bin/env bash
# mac-testflight.sh — the Mac build, on TestFlight.
#
# The same place the phone build goes: macOS apps use TestFlight too,
# and this target carries the phone's bundle identifier so the two are
# one app record with a build per platform (Apple's universal
# purchase) rather than two apps sharing a name.
#
# Usage:
#   ./scripts/mac-testflight.sh <version>
#
# Authentication is Xcode's own signed-in account — the same path the
# iOS upload takes. No API key is stored here; if the account cannot
# reach the record, the upload says so by name rather than failing
# vaguely.
#
# Direct distribution (a signed, notarised .dmg somebody downloads) is
# a different path with a different certificate. It is not this.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="${1:?usage: mac-release.sh <version>}"
TEAM=KF79DRC524
OUT="/tmp/mailrs-mac-release"
ARCHIVE="$OUT/Mailrs.xcarchive"

say() { printf '==> %s\n' "$*"; }
die() { printf '!! %s\n' "$*" >&2; exit 1; }

say "[0/4] the certificate this needs"
security find-identity -v -p codesigning 2>/dev/null | grep -q "Apple Distribution: GOLIA K.K." \
    || echo "!! no 'Apple Distribution' identity yet — Xcode will ask for one during the archive"

rm -rf "$OUT"
mkdir -p "$OUT"

say "[1/4] xcodegen"
(cd ios && xcodegen generate --spec project.yml >/dev/null)

say "[2/4] archive $VERSION"
# The build number has to rise for every upload of the same version:
# App Store Connect refuses one it has already seen, and it says so
# only after the upload has finished.
(cd ios && xcodebuild -project Mailrs.xcodeproj -scheme MailrsMac \
    -configuration Release -destination "platform=macOS" \
    -archivePath "$ARCHIVE" \
    MARKETING_VERSION="$VERSION" CURRENT_PROJECT_VERSION="${BUILD:-1}" \
    -allowProvisioningUpdates archive) > "$OUT/archive.log" 2>&1 \
    || { grep -E "error:" "$OUT/archive.log" | sort -u | head; die "archive failed: $OUT/archive.log"; }

say "[3/4] export and upload"
cat > "$OUT/export.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key><string>app-store-connect</string>
    <key>teamID</key><string>$TEAM</string>
    <key>destination</key><string>upload</string>
    <key>uploadSymbols</key><true/>
</dict>
</plist>
PLIST
(cd ios && xcodebuild -exportArchive -archivePath "$ARCHIVE" \
    -exportPath "$OUT/upload" -exportOptionsPlist "$OUT/export.plist" \
    -allowProvisioningUpdates) > "$OUT/upload.log" 2>&1 \
    || {
        # The reason is in the distribution logs, not in xcodebuild's
        # own output — a missing app record reports as
        # `missingApp(bundleId:)` there and as one opaque line here.
        grep -iE "error|missingApp|Invalid" "$OUT/upload.log" | sort -u | head -5
        LOGS=$(ls -dt /var/folders/*/*/T/Mailrs*.xcdistributionlogs 2>/dev/null | head -1)
        [ -n "$LOGS" ] && grep -ihE "step failed|error" "$LOGS"/*.log 2>/dev/null | tail -5
        die "upload failed: $OUT/upload.log"
    }
grep -q "Upload succeeded" "$OUT/upload.log" || die "no upload confirmation in $OUT/upload.log"

say "[4/4] uploaded — App Store Connect is processing it"
grep -E "Upload succeeded|Uploaded" "$OUT/upload.log" | tail -2
