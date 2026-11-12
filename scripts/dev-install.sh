#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Installs the desktop entry of Kropa for the current user, without sudo.
#
# The portals (screenshot permission, GlobalShortcuts) know an application by its identifier, which
# is the name of its desktop file: io.github.fs0ciety7000.Kropa.desktop. `kropa` registers that
# identifier with the portal at each capture, and GNOME asks "Allow Kropa to take screenshots?"
# with the name from this file. Run this once after building, and again if the binary moves.
#
# Usage: scripts/dev-install.sh [--bin PATH] [--uninstall]
#   --bin PATH    kropa binary the entry launches (default: target/release/kropa, then
#                 target/debug/kropa of this checkout, then `kropa` found in PATH)
#   --uninstall   remove the entry
#
# Idempotent: running it again with the same binary changes nothing.

set -eu

APP_ID="io.github.fs0ciety7000.Kropa"

repo=$(cd "$(dirname "$0")/.." && pwd)
source_file="$repo/data/$APP_ID.desktop"
data_home=${XDG_DATA_HOME:-${HOME:?HOME is not set}/.local/share}
apps_dir="$data_home/applications"
dest="$apps_dir/$APP_ID.desktop"

bin=""
uninstall=0
while [ $# -gt 0 ]; do
    case "$1" in
        --bin)
            [ $# -ge 2 ] || { echo "dev-install: --bin needs a path" >&2; exit 2; }
            bin=$2
            shift 2
            ;;
        --uninstall)
            uninstall=1
            shift
            ;;
        -h | --help)
            sed -n '4,17p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "dev-install: unknown option: $1" >&2
            exit 2
            ;;
    esac
done

refresh_cache() {
    # Not needed for the portals (they read the file itself); keeps launchers in sync when present.
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$apps_dir" >/dev/null 2>&1 || true
    fi
}

if [ "$uninstall" -eq 1 ]; then
    if [ -e "$dest" ]; then
        rm -f -- "$dest"
        refresh_cache
        echo "removed $dest"
    else
        echo "nothing to remove: $dest"
    fi
    exit 0
fi

[ -f "$source_file" ] || { echo "dev-install: $source_file not found" >&2; exit 1; }

if [ -z "$bin" ]; then
    for candidate in "$repo/target/release/kropa" "$repo/target/debug/kropa"; do
        if [ -x "$candidate" ]; then
            bin=$candidate
            break
        fi
    done
fi
if [ -z "$bin" ]; then
    bin=$(command -v kropa || true)
fi
if [ -z "$bin" ] || [ ! -x "$bin" ]; then
    echo "dev-install: no kropa binary found; run 'cargo build -p kropa' or pass --bin PATH" >&2
    exit 1
fi
# An absolute path: the entry must not depend on the PATH of whoever launches it.
case "$bin" in
    /*) ;;
    *) bin="$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")" ;;
esac
case "$bin" in
    *[[:space:]\"\\]*)
        echo "dev-install: the path of the binary must not contain spaces, quotes or backslashes: $bin" >&2
        exit 1
        ;;
esac

mkdir -p -- "$apps_dir"
# A private folder next to the destination (same file system, so the final mv is atomic), and a
# file named *.desktop inside it: desktop-file-validate refuses any other extension.
tmpdir=$(mktemp -d "$apps_dir/.$APP_ID.XXXXXX")
trap 'rm -rf -- "$tmpdir"' EXIT HUP INT TERM
tmp="$tmpdir/$APP_ID.desktop"
# `|` separates the sed expression below: a path holding it is refused rather than escaped.
case "$bin" in
    *'|'*) echo "dev-install: unsupported character '|' in $bin" >&2; exit 1 ;;
esac
sed -e "s|^TryExec=.*|TryExec=$bin|" -e "s|^Exec=kropa |Exec=$bin |" "$source_file" >"$tmp"
chmod 0644 "$tmp"

if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$tmp"
fi

if [ -f "$dest" ] && cmp -s "$tmp" "$dest"; then
    echo "up to date: $dest"
else
    mv -f -- "$tmp" "$dest"
    refresh_cache
    echo "installed $dest (launches $bin)"
fi
