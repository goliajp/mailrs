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

# **No local certificate check.** This used to warn when the keychain
# held no "Apple Distribution" identity, and that warning was printed on
# every single run — including the ones that uploaded — because the
# distribution certificate is **cloud-managed**: Apple keeps it and the
# private key, Xcode signs through the account session at export time,
# and nothing lands in this machine's keychain. Measured on the 1.0.0
# (4) upload: `security find-certificate -c "Apple Distribution"` finds
# nothing while the export log names the certificate it used.
#
# What actually has to be true is the **Xcode account session**, which
# no local query can see. When it has expired the archive fails with
# "no signing certificate 'Mac App Distribution' found" — which reads
# as a missing certificate and is not one. Sign in again at Xcode →
# Settings → Apple Accounts and re-run.

rm -rf "$OUT"
mkdir -p "$OUT"

say "[1/5] xcodegen"
(cd ios && xcodegen generate --spec project.yml >/dev/null)

say "[2/5] archive $VERSION"
# The build number has to rise for every upload of the same version:
# App Store Connect refuses one it has already seen, and it says so
# only after the upload has finished.
(cd ios && xcodebuild -project Mailrs.xcodeproj -scheme MailrsMac \
    -configuration Release -destination "platform=macOS" \
    -archivePath "$ARCHIVE" \
    MARKETING_VERSION="$VERSION" CURRENT_PROJECT_VERSION="${BUILD:-1}" \
    -allowProvisioningUpdates archive) > "$OUT/archive.log" 2>&1 \
    || { grep -E "error:" "$OUT/archive.log" | sort -u | head; die "archive failed: $OUT/archive.log"; }

# **Exported to a file first, and looked at.** The iOS lane has done
# this from the start; this one went straight to upload, so nothing
# ever checked what was being sent. A build signed for development
# uploads and is rejected afterwards, by mail, which is a slow way to
# find out.
say "[3/5] export"
cat > "$OUT/check.plist" <<PLIST
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
    -exportPath "$OUT/check" -exportOptionsPlist "$OUT/check.plist" \
    -allowProvisioningUpdates) > "$OUT/check.log" 2>&1 \
    || die "export failed: $OUT/check.log"

say "[4/5] check what is about to be sent"
# The installer certificate, on the package itself.
pkgutil --check-signature "$OUT/check/MailrsMac.pkg" 2>&1 \
    | grep -q "3rd Party Mac Developer Installer" \
    || die "the package is not signed for the App Store"
# And the app inside it. `2>&1` matters: codesign writes this to
# **stderr**, and grepping stdout finds nothing and reports a correctly
# signed build as unsigned — a gate that fails a good release is the
# kind people delete.
rm -rf "$OUT/expanded"
pkgutil --expand-full "$OUT/check/MailrsMac.pkg" "$OUT/expanded" >/dev/null 2>&1 \
    || die "could not open the package to look inside it"
INNER=$(find "$OUT/expanded" -maxdepth 5 -name '*.app' -type d | head -1)
[ -n "$INNER" ] || die "no app inside the package"
SIGNED=$(codesign -dvv "$INNER" 2>&1 | grep -E "^Authority=" | head -1)
case "$SIGNED" in
    *"Apple Distribution"*) : ;;
    *) die "not signed for distribution — it is: ${SIGNED:-<codesign said nothing>}" ;;
esac
say "checked: Apple Distribution, App Store installer signature"

say "[5/5] export and upload"
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

say "uploaded — App Store Connect is processing it"
grep -E "Upload succeeded|Uploaded" "$OUT/upload.log" | tail -2
