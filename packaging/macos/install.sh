#!/bin/sh
# Builds YTFast on this Mac and puts it in Applications.
#
# The app is signed with a signature of this Mac's own ("YTFast Local
# Signing", made the first time this runs and kept in the login keychain),
# so macOS takes every build for the same app and keeps what it was
# allowed: Full Disk Access, which reading Safari's sign-in needs. CI's
# builds are signed "ad hoc" instead, and are a new app to macOS each time.
#
# The first time, macOS asks for the Mac's password so that codesign may
# use the signature: choose Always Allow.
#
# The packaging is CI's "Package (macOS)" step: keep the two in step.
set -eu

identity="YTFast Local Signing"
cd "$(dirname "$0")/../.."
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

if ! security find-certificate -c "$identity" > /dev/null 2>&1; then
    echo "Making this Mac's signature, \"$identity\"..."
    cat > "$work/cert.conf" <<EOF
[req]
distinguished_name = name
x509_extensions = use
prompt = no
[name]
CN = $identity
[use]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF
    # macOS's own openssl: its files are the kind the keychain reads.
    /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
        -config "$work/cert.conf" -keyout "$work/key.pem" -out "$work/cert.pem" 2> /dev/null
    # This password protects the file below for the moment it exists.
    pass=$(/usr/bin/openssl rand -hex 16)
    /usr/bin/openssl pkcs12 -export -name "$identity" -inkey "$work/key.pem" \
        -in "$work/cert.pem" -out "$work/identity.p12" -passout "pass:$pass"
    # -x: the key cannot be copied out of the keychain again.
    security import "$work/identity.p12" -P "$pass" -T /usr/bin/codesign -x
fi

cargo build --release --locked -p ytfast

app="$work/YTFast.app"
iconset="$work/YTFast.iconset"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$iconset"
cp target/release/ytfast "$app/Contents/MacOS/ytfast"
cp packaging/macos/Info.plist "$app/Contents/Info.plist"
for size in 16 32 128 256 512; do
    double=$((size * 2))
    sips -z "$size" "$size" packaging/icons/macos-1024.png --out "$iconset/icon_${size}x${size}.png" > /dev/null
    sips -z "$double" "$double" packaging/icons/macos-1024.png --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/YTFast.icns"
codesign --force --deep --sign "$identity" "$app"
codesign --verify --deep --strict "$app"

rm -rf /Applications/YTFast.app
mv "$app" /Applications/YTFast.app
echo "YTFast is in Applications. If it was open, quit it and open it again."
