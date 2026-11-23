#!/bin/bash
# Build the Android app (arm64, Android 8.0+) without Gradle:
# cargo-ndk for the Rust viewer, javac + d8 for MainActivity, aapt2 for resources, then
# zipalign and apksigner.
#
#   ./build.sh               -> $OUT/schemview.apk
#   ./build.sh install       -> also `adb install -r` it on the connected phone
#   ABIS="arm64-v8a x86_64" ./build.sh   -> also include x86_64 (for the emulator)
#
# Needs: Android SDK (build-tools 36, platform android-36, an NDK), cargo-ndk, the Rust
# target aarch64-linux-android, a JDK. The release key is created on the first run in
# ~/.android/schemview-release.jks; keep it, updates must be signed with the same key.
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

SDK=${ANDROID_HOME:-$HOME/Android/Sdk}
BT=$SDK/build-tools/36.0.0
PLATFORM=$SDK/platforms/android-36/android.jar
NDK=${ANDROID_NDK_HOME:-$(ls -d "$SDK"/ndk/* | sort -V | tail -1)}
OUT=${OUT:-/mnt/archive/Rust_Projects/schematic-viewer/android}
KS=${KEYSTORE:-$HOME/.android/schemview-release.jks}
KSPASS=${KS%.jks}.pass
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' native/Cargo.toml | head -1)
# Increases with every build (minutes since 2026-01-01), so a new APK always installs over the old one.
CODE=$(( ($(date +%s) - 1767225600) / 60 ))

for f in "$BT/aapt2" "$BT/d8" "$BT/zipalign" "$BT/apksigner" "$PLATFORM" "$NDK/source.properties"; do
    [ -e "$f" ] || { echo "missing: $f" >&2; exit 1; }
done

echo "== schemview $VERSION ($CODE), NDK $(basename "$NDK")"
ABIS=${ABIS:-arm64-v8a}
rm -rf "$OUT/stage" && mkdir -p "$OUT/stage"/{classes,dex,lib}

echo "== Rust ($ABIS)"
targets=(); for a in $ABIS; do targets+=(-t "$a"); done
ANDROID_NDK_HOME=$NDK cargo ndk "${targets[@]}" -P 26 -o "$OUT/stage/lib" \
    build --release --manifest-path native/Cargo.toml
for a in $ABIS; do
    [ -f "$OUT/stage/lib/$a/libschemview_android.so" ] || { echo "cargo-ndk produced no $a library" >&2; exit 1; }
done

echo "== resources"
"$BT/aapt2" compile --dir res -o "$OUT/stage/res.zip"
"$BT/aapt2" link -o "$OUT/stage/base.apk" -I "$PLATFORM" --manifest AndroidManifest.xml \
    --min-sdk-version 26 --target-sdk-version 36 --version-code "$CODE" --version-name "$VERSION" \
    "$OUT/stage/res.zip"

echo "== Java"
javac -Xlint:-options -source 8 -target 8 -bootclasspath "$PLATFORM:$BT/core-lambda-stubs.jar" -d "$OUT/stage/classes" \
    $(find java -name '*.java')
"$BT/d8" --release --min-api 26 --lib "$PLATFORM" --output "$OUT/stage/dex" \
    $(find "$OUT/stage/classes" -name '*.class')

echo "== package"
cp "$OUT/stage/base.apk" "$OUT/stage/unsigned.apk"
(cd "$OUT/stage/dex" && zip -q -X "$OUT/stage/unsigned.apk" classes.dex)
# Native code stays uncompressed and 16 KB aligned, so Android maps it straight from the APK.
(cd "$OUT/stage" && zip -q -X -0 -r unsigned.apk lib)
"$BT/zipalign" -f -P 16 4 "$OUT/stage/unsigned.apk" "$OUT/stage/aligned.apk"

if [ ! -f "$KS" ]; then
    echo "== creating release key $KS"
    mkdir -p "$(dirname "$KS")"
    (umask 077; head -c 24 /dev/urandom | base64 | tr -d '/+=' > "$KSPASS")
    keytool -genkeypair -keystore "$KS" -storepass:file "$KSPASS" -alias schemview \
        -keyalg RSA -keysize 4096 -validity 36500 -dname "CN=schemview" >/dev/null
    chmod 600 "$KS"
fi
"$BT/apksigner" sign --ks "$KS" --ks-pass "file:$KSPASS" --ks-key-alias schemview \
    --out "$OUT/schemview.apk" "$OUT/stage/aligned.apk"
"$BT/apksigner" verify "$OUT/schemview.apk"
"$BT/zipalign" -c -P 16 4 "$OUT/schemview.apk"
ls -la "$OUT/schemview.apk"

if [ "${1:-}" = install ]; then
    echo "== install"
    adb install -r "$OUT/schemview.apk"
fi
