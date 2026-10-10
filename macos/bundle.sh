#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail

NAME="Notepad++ for macOS (unofficial)"
SHORT_NAME="Notepad++"
BUNDLE_ID="io.github.azwanzuharimi.notepadpp-mac"
EXE="notepadpp-mac"
SOURCE_URL="https://github.com/azwanzuharimi/notepad-plus-plus"

cd "$(dirname "$0")"
ROOT=..
OUT=target/bundle
APP="$OUT/$NAME.app"

cargo build --release
VERSION=$(cargo metadata --offline --no-deps --format-version 1 | jq -r --arg n "$EXE" '.packages[] | select(.name == $n) | .version')
MINOS=$(otool -l "target/release/$EXE" | awk '/minos/ { print $2; exit }')

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/release/$EXE" "$APP/Contents/MacOS/"
PLIST="$APP/Contents/Info.plist"
cp bundle/Info.plist "$PLIST"
for kv in "CFBundleExecutable=$EXE" "CFBundleIdentifier=$BUNDLE_ID" "CFBundleName=$SHORT_NAME" \
    "CFBundleDisplayName=$NAME" "CFBundleShortVersionString=$VERSION" "CFBundleVersion=$VERSION" \
    "LSMinimumSystemVersion=$MINOS" \
    "NSHumanReadableCopyright=GPL-3.0-or-later. Source: $SOURCE_URL. Not affiliated with the Notepad++ project."; do
    plutil -replace "${kv%%=*}" -string "${kv#*=}" "$PLIST"
done
plutil -convert xml1 "$PLIST"
if grep -q '@[A-Z_]*@' "$PLIST"; then echo "Placeholder left in Info.plist" >&2; exit 1; fi

RES="$APP/Contents/Resources"
for c in busy flipped; do
    tiffutil -cathidpicheck "$ROOT/scintilla/cocoa/res/mac_cursor_$c.png" \
        "$ROOT/scintilla/cocoa/res/mac_cursor_$c@2x.png" -out "$RES/mac_cursor_$c.tiff" >/dev/null
done

ICONSET="$OUT/AppIcon.iconset"
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
    sips -z $s $s bundle/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
    sips -z $((s * 2)) $((s * 2)) bundle/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$RES/AppIcon.icns"

cp "$ROOT/LICENSE" "$RES/LICENSE.txt"
cp "$ROOT/scintilla/License.txt" "$RES/Scintilla-License.txt"
cp "$ROOT/lexilla/License.txt" "$RES/Lexilla-License.txt"
cp "$ROOT/scintilla/test/unit/LICENSE_1_0.txt" "$RES/Boost-LICENSE_1_0.txt"
cat >"$RES/README.txt" <<EOF
$NAME $VERSION

An unofficial, modified version of Notepad++ for macOS.
Not affiliated with the Notepad++ project.

Source code: $SOURCE_URL
Licence: GPL-3.0-or-later (see LICENSE.txt).
Scintilla, Lexilla, and Boost: see Scintilla-License.txt, Lexilla-License.txt, and Boost-LICENSE_1_0.txt.
Rust crates: see THIRD_PARTY_LICENSES.txt.
EOF

META=$(cargo metadata --offline --format-version 1)
{
    echo "Third party licences for the Rust crates in $NAME."
    echo "Where a crate gives a choice of licences, this app uses the crate under the MIT licence."
    cargo tree --offline -e normal --prefix none --format '{p}' |
        awk -v exe="$EXE" '$1 != exe { print $1 "\t" substr($2, 2) }' | sort -u |
        while IFS=$'\t' read -r name ver; do
            IFS=$'\t' read -r lic manifest repo < <(jq -r --arg n "$name" --arg v "$ver" \
                '.packages[] | select(.name == $n and .version == $v) | [.license, .manifest_path, .repository] | @tsv' <<<"$META")
            files=()
            while IFS= read -r f; do files+=("$f"); done < <(find "$(dirname "$manifest")" -maxdepth 1 -type f \
                \( -iname 'LICEN[CS]E*' -o -iname 'COPYING*' -o -iname 'UNLICENSE*' -o -iname 'NOTICE*' \) | sort)
            if [ ${#files[@]} -eq 0 ] && [ -d "bundle/licenses/$(basename "$repo")" ]; then
                while IFS= read -r f; do files+=("$f"); done < <(find "bundle/licenses/$(basename "$repo")" -type f | sort)
            fi
            [ ${#files[@]} -gt 0 ] || { echo "No licence file for $name $ver" >&2; exit 1; }
            printf '\n================================================================\n%s %s (%s)\n%s\n' "$name" "$ver" "$lic" "$repo"
            for f in "${files[@]}"; do
                printf -- '\n---- %s ----\n\n' "$(basename "$f")"
                cat "$f"
            done
        done
} >"$RES/THIRD_PARTY_LICENSES.txt"

codesign --force --deep -s - "$APP"
codesign --verify --deep --strict "$APP"
echo "$APP"
