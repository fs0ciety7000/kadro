#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds Kropa.app: the `kropa` binary in Contents/MacOS, Info.plist, localized strings, signature.
#
# macOS files its permissions (Screen Recording, Keychain) under an application, not under a bare
# binary, so the command line lives inside a bundle (docs/platform-macos.md 4.2). Put a link to
# Kropa.app/Contents/MacOS/kropa in your PATH to use it.
#
# Usage: scripts/bundle-macos.sh [options]
#   --out DIR            folder that receives Kropa.app (default: target/bundle of this checkout)
#   --bin PATH           use this binary instead of building one (required off macOS)
#   --universal          build for aarch64 and x86_64 and join them with lipo (needs both rustup
#                        targets: rustup target add aarch64-apple-darwin x86_64-apple-darwin)
#   --profile NAME       cargo profile to build with (default: release; dev for a quick check)
#   --sign IDENTITY      codesign identity: "-" for an ad hoc signature (default), or the name or
#                        SHA-1 of a certificate that is already in a keychain
#   --keychain PATH      keychain that holds IDENTITY (default: the search list of the user)
#   --hardened-runtime   sign with the hardened runtime (--options runtime)
#   --no-sign            assemble the bundle only (checks of the layout on any machine)
#   --version X.Y.Z      version written to Info.plist (default: the workspace version)
#
# The script never reads a private key: codesign finds the identity itself, and macOS asks the
# user for access to the key if it needs to. Creating, storing and backing up the certificate is
# not done here (docs/plan.md, task 1.6).
#
# An ad hoc signature ties the permissions to this exact build: macOS asks for them again after
# every rebuild. Sign with the project certificate to keep them (docs/platform-macos.md 4.3).
#
# Nothing here touches the network: --timestamp=none keeps codesign from calling Apple's server.

set -eu

APP_ID="io.github.fs0ciety7000.Kropa"
APP_NAME="Kropa.app"

repo=$(cd "$(dirname "$0")/.." && pwd)
data="$repo/data/macos"

out="$repo/target/bundle"
bin=""
universal=0
profile="release"
identity="-"
keychain=""
runtime=0
sign=1
version=""

die() {
    echo "bundle-macos: $*" >&2
    exit 1
}

need_value() {
    [ "$2" -ge 2 ] || die "$1 needs a value"
}

while [ $# -gt 0 ]; do
    case "$1" in
        --out) need_value "$1" $#; out=$2; shift 2 ;;
        --bin) need_value "$1" $#; bin=$2; shift 2 ;;
        --universal) universal=1; shift ;;
        --profile) need_value "$1" $#; profile=$2; shift 2 ;;
        --sign) need_value "$1" $#; identity=$2; shift 2 ;;
        --keychain) need_value "$1" $#; keychain=$2; shift 2 ;;
        --hardened-runtime) runtime=1; shift ;;
        --no-sign) sign=0; shift ;;
        --version) need_value "$1" $#; version=$2; shift 2 ;;
        -h | --help)
            sed -n '4,30p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) die "unknown option: $1" ;;
    esac
done

os=$(uname -s)
if [ "$sign" -eq 1 ] && [ "$os" != "Darwin" ]; then
    die "codesign needs macOS; use --no-sign to assemble the layout only"
fi
if [ "$universal" -eq 1 ] && [ -n "$bin" ]; then
    die "--universal builds its own binary; do not combine it with --bin"
fi

if [ -z "$version" ]; then
    version=$(awk -F'"' '/^\[workspace\.package\]/ { in_section = 1; next } /^\[/ { in_section = 0 } in_section && /^version = / { print $2; exit }' "$repo/Cargo.toml")
fi
# CFBundleShortVersionString is made of up to three dot-separated numbers.
case "$version" in
    '' | *[!0-9.]* | .* | *. | *..*) die "version '$version' is not of the form X.Y.Z" ;;
esac

# --- the binary ---------------------------------------------------------------------------------

work=$(mktemp -d "${TMPDIR:-/tmp}/kropa-bundle.XXXXXX")
trap 'rm -rf -- "$work"' EXIT HUP INT TERM

if [ -z "$bin" ]; then
    [ "$os" = "Darwin" ] || die "building a macOS binary needs macOS; pass one with --bin PATH"
    command -v cargo >/dev/null 2>&1 || die "cargo not found"
    case "$profile" in
        dev) profile_dir=debug ;;
        *) profile_dir=$profile ;;
    esac
    build() { # build TRIPLE
        (cd "$repo" && cargo build --locked --profile "$profile" -p kropa --target "$1") >&2
    }
    if [ "$universal" -eq 1 ]; then
        build aarch64-apple-darwin
        build x86_64-apple-darwin
        command -v lipo >/dev/null 2>&1 || die "lipo not found (install the Xcode command line tools)"
        lipo -create -output "$work/kropa" \
            "$repo/target/aarch64-apple-darwin/$profile_dir/kropa" \
            "$repo/target/x86_64-apple-darwin/$profile_dir/kropa"
        bin="$work/kropa"
    else
        (cd "$repo" && cargo build --locked --profile "$profile" -p kropa) >&2
        bin="$repo/target/$profile_dir/kropa"
    fi
fi
[ -f "$bin" ] && [ -x "$bin" ] || die "no executable binary at $bin"

# --- the bundle ---------------------------------------------------------------------------------

app="$out/$APP_NAME"
mkdir -p -- "$out"
rm -rf -- "$app"
mkdir -p -- "$app/Contents/MacOS" "$app/Contents/Resources/en.lproj" "$app/Contents/Resources/fr.lproj"
cp -- "$bin" "$app/Contents/MacOS/kropa"
chmod 0755 "$app/Contents/MacOS/kropa"
sed "s/@VERSION@/$version/g" "$data/Info.plist" >"$app/Contents/Info.plist"
cp -- "$data/en.lproj/InfoPlist.strings" "$app/Contents/Resources/en.lproj/InfoPlist.strings"
cp -- "$data/fr.lproj/InfoPlist.strings" "$app/Contents/Resources/fr.lproj/InfoPlist.strings"
chmod 0644 "$app/Contents/Info.plist" "$app/Contents/Resources/en.lproj/InfoPlist.strings" \
    "$app/Contents/Resources/fr.lproj/InfoPlist.strings"

if grep -q '@VERSION@' "$app/Contents/Info.plist"; then
    die "Info.plist still holds a placeholder"
fi
if command -v plutil >/dev/null 2>&1; then
    plutil -lint "$app/Contents/Info.plist" >&2
    plutil -lint "$app/Contents/Resources/en.lproj/InfoPlist.strings" \
        "$app/Contents/Resources/fr.lproj/InfoPlist.strings" >&2
fi

# --- the signature ------------------------------------------------------------------------------

if [ "$sign" -eq 1 ]; then
    set -- --force --sign "$identity" --identifier "$APP_ID" --timestamp=none
    if [ -n "$keychain" ]; then
        set -- "$@" --keychain "$keychain"
    fi
    if [ "$runtime" -eq 1 ]; then
        set -- "$@" --options runtime
    fi
    codesign "$@" "$app" >&2
    codesign --verify --strict --verbose=2 "$app" >&2
    {
        echo "signature of $APP_NAME:"
        codesign --display --verbose=2 "$app" 2>&1 | grep -E '^(Identifier|Format|CodeDirectory|Signature|Authority|CDHash)' | sed 's/^/  /'
        # The designated requirement is what TCC records. With "-" it holds the cdhash of this build
        # (permissions are lost at the next build); with a certificate it holds the certificate.
        codesign --display --requirements - "$app" 2>&1 | grep '^designated => ' | sed 's/^designated => /  designated requirement: /'
    } >&2
    if [ "$identity" = "-" ]; then
        echo "bundle-macos: ad hoc signature: macOS will ask again for the permissions after the next build." >&2
    fi
else
    echo "bundle-macos: not signed (--no-sign): macOS will not keep any permission for this bundle." >&2
fi

echo "$app"
