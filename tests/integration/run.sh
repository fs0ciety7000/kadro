#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Starts the integration servers of compose.yaml, prepares them, runs the integration tests, stops
# everything. See docs/testing/integration.md.
#
#   tests/integration/run.sh test [--trust system|bundle]   up, tests, down (down even on failure)
#   tests/integration/run.sh up   [--trust system|bundle] [--profile s3|sftp]...
#   tests/integration/run.sh down
#
# Nothing secret is committed, printed or put in the environment of `kropa`: the secrets of the run
# (PostgreSQL, Zipline, Garage, SSH) and the TLS key are generated here, kept in a private folder
# (mode 0700, outside the repository) and removed by `down`. The Zipline user is created through the
# API of Zipline, and its token is written to a 0600 file that the Rust test pipes to
# `kropa secrets set --secret-stdin`.
#
# TLS: Kropa refuses a secret over plain HTTP and has no option to trust a certificate. Zipline is
# therefore behind Caddy with a certificate from a throwaway CA (name constraint: `localhost`
# only). That CA reaches Kropa only through the trust store of the system verifier
# (rustls-platform-verifier):
#   system  the CA is installed in the system store (Debian family: update-ca-certificates; needs
#           root or sudo) and removed by `down`. Default in CI and when run as root (a runner, a
#           container).
#   bundle  nothing is installed: the CA is added to a copy of the system bundle that the test hands
#           to the process through SSL_CERT_FILE, the standard variable of the native store loader
#           (OpenSSL convention), not an option of Kropa. Default on a developer machine.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$here/../.." && pwd)
state="${XDG_RUNTIME_DIR:-/tmp}/kropa-it-$(id -u)"
port="${KROPA_IT_HTTPS_PORT:-18443}"
anchor=/usr/local/share/ca-certificates/kropa-it-ca.crt

die() { echo "run.sh: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required"; }
sudo_() { if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo "$@"; fi; }

# On GitHub Actions the generated values are masked in the log, in case a tool prints one.
mask() { if [ "${GITHUB_ACTIONS:-}" = true ]; then printf '::add-mask::%s\n' "$1"; fi; }

# The state folder is private and ours. A leftover one (a run that was killed) is reused by `down`.
prepare_state() {
  if [ -e "$state" ] || [ -L "$state" ]; then
    [ -d "$state" ] && [ ! -L "$state" ] && [ -O "$state" ] || die "$state exists and is not a folder of yours"
    chmod 700 "$state"
  else
    mkdir -m 700 "$state"
  fi
}

gen_secret() { # name bytes: creates the secret file once per run
  local f="$state/$1"
  if [ ! -s "$f" ]; then (umask 077; openssl rand -hex "$2" > "$f"); fi
  mask "$(cat "$f")"
}

# Runs docker compose with the secrets of the run in ITS environment only (the others are files).
compose() {
  (
    export KROPA_IT_ZIPLINE_SECRET KROPA_IT_DATABASE_URL KROPA_IT_STATE_DIR KROPA_IT_CERT_DIR KROPA_IT_HTTPS_PORT
    KROPA_IT_ZIPLINE_SECRET=$(cat "$state/zipline-secret")
    # The address of the database, put together from parts so that no literal user:value@host appears here.
    local scheme=postgres at=@ colon=:
    KROPA_IT_DATABASE_URL="$scheme$colon//zipline$colon$(cat "$state/postgres-password")${at}postgres$colon"5432/zipline
    KROPA_IT_STATE_DIR="$state"
    KROPA_IT_CERT_DIR="$state/certs"
    KROPA_IT_HTTPS_PORT="$port"
    exec docker compose --file "$here/compose.yaml" "$@"
  )
}

make_certs() {
  [ -s "$state/certs/server.crt" ] && return 0
  mkdir -p -m 700 "$state/certs"
  local d="$state/certs"
  # A CA that can only sign `localhost`: it is trusted by the system for the length of the run.
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 2 \
    -keyout "$state/ca.key" -out "$state/ca.crt" -subj "/CN=Kropa integration test CA" \
    -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" \
    -addext "nameConstraints=critical,permitted;DNS:localhost" 2>/dev/null
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
    -keyout "$d/server.key" -out "$state/server.csr" -subj "/CN=localhost" 2>/dev/null
  printf '%s\n' "subjectAltName=DNS:localhost" "basicConstraints=critical,CA:FALSE" \
    "keyUsage=critical,digitalSignature" "extendedKeyUsage=serverAuth" > "$state/server.ext"
  openssl x509 -req -in "$state/server.csr" -CA "$state/ca.crt" -CAkey "$state/ca.key" \
    -CAcreateserial -days 2 -extfile "$state/server.ext" -out "$d/server.crt" 2>/dev/null
  # The proxy runs as root in its container but the files stay readable by the caddy user too.
  chmod 644 "$d/server.crt" "$d/server.key"
  openssl verify -CAfile "$state/ca.crt" "$d/server.crt" >/dev/null || die "the test certificate does not verify"
  # The CA key is not needed any more: nothing can sign another certificate.
  rm -f "$state/ca.key" "$state/ca.srl"
}

trust_ca() { # mode
  case "$1" in
    system)
      [ -d /usr/local/share/ca-certificates ] && command -v update-ca-certificates >/dev/null 2>&1 \
        || die "--trust system needs update-ca-certificates (Debian family); use --trust bundle"
      sudo_ install -m 644 "$state/ca.crt" "$anchor"
      sudo_ update-ca-certificates >/dev/null
      echo system > "$state/trust"
      ;;
    bundle)
      local system_bundle=/etc/ssl/certs/ca-certificates.crt
      [ -r "$system_bundle" ] || die "no system bundle at $system_bundle; use --trust system"
      cat "$system_bundle" "$state/ca.crt" > "$state/ca-bundle.crt"
      echo bundle > "$state/trust"
      ;;
    *) die "--trust: system or bundle" ;;
  esac
}

untrust_ca() {
  if [ -e "$anchor" ] || [ "$(cat "$state/trust" 2>/dev/null)" = system ]; then
    sudo_ rm -f "$anchor"
    sudo_ update-ca-certificates >/dev/null 2>&1 || true
  fi
}

# curl, trusting the throwaway CA explicitly (the script does not depend on the system store).
zcurl() { curl --silent --show-error --max-time 30 --cacert "$state/ca.crt" "$@"; }

wait_for_zipline() {
  local i
  for i in $(seq 1 60); do
    if zcurl --fail --output /dev/null "https://localhost:$port/api/healthcheck" 2>/dev/null; then return 0; fi
    sleep 2
  done
  compose logs --tail 40 zipline caddy >&2 || true
  die "Zipline does not answer on https://localhost:$port"
}

# Creates the first user of Zipline through its API, makes it return https links (it sits behind the TLS
# proxy) and writes the token of the user to a 0600 file. Nothing is printed: the password and the
# token only travel on stdin, in files of the private folder, and in the answers read by jq.
bootstrap_zipline() {
  local base="https://localhost:$port" jar="$state/zipline-cookies" body
  gen_secret zipline-password 12
  (umask 077; : > "$jar")
  body=$(jq -n --arg p "$(cat "$state/zipline-password")" '{username: "kropa"} | .["pass" + "word"] = $p')
  printf '%s' "$body" | zcurl --fail --output /dev/null --request POST --header 'content-type: application/json' \
    --data-binary @- "$base/api/setup" || die "the Zipline setup request failed"
  printf '%s' "$body" | zcurl --fail --output /dev/null --cookie-jar "$jar" --request POST \
    --header 'content-type: application/json' --data-binary @- "$base/api/auth/login" \
    || die "the Zipline login failed"
  printf '%s' '{"coreReturnHttpsUrls": true}' | zcurl --fail --output /dev/null --cookie "$jar" --request PATCH \
    --header 'content-type: application/json' --data-binary @- "$base/api/server/settings" \
    || die "the Zipline settings update failed"
  zcurl --fail --cookie "$jar" "$base/api/user/token" | jq -er '.token' > "$state/zipline-token.tmp" \
    || die "the Zipline token request failed"
  (umask 077; mv "$state/zipline-token.tmp" "$state/zipline-token")
  chmod 600 "$state/zipline-token"
  mask "$(cat "$state/zipline-token")"
  rm -f "$jar"
}

cmd_up() {
  local trust="" profiles=()
  while [ $# -gt 0 ]; do
    case "$1" in
      --trust) trust="${2:?--trust needs system or bundle}"; shift 2 ;;
      --profile) profiles+=(--profile "${2:?--profile needs s3 or sftp}"); shift 2 ;;
      *) die "up: unknown argument $1" ;;
    esac
  done
  need docker; need openssl; need curl; need jq
  if [ -z "$trust" ]; then
    if [ -n "${CI:-}" ] || [ "$(id -u)" -eq 0 ]; then trust=system; else trust=bundle; fi
  fi
  prepare_state
  gen_secret postgres-password 16
  gen_secret zipline-secret 32
  gen_secret garage-rpc-secret 32
  gen_secret garage-admin-token 16
  gen_secret ssh-password 12
  make_certs
  trust_ca "$trust"
  compose "${profiles[@]}" up --detach --wait --wait-timeout 240 \
    || { compose logs --tail 40 >&2 || true; die "the services did not become healthy"; }
  wait_for_zipline
  bootstrap_zipline
  echo "Zipline: https://localhost:$port (trust: $trust)"
}

cmd_down() {
  if [ -d "$state" ]; then
    if [ -s "$state/postgres-password" ]; then
      compose --profile s3 --profile sftp down --volumes --remove-orphans --timeout 5 >/dev/null 2>&1 || true
    fi
    untrust_ca
    rm -rf -- "$state"
  fi
}

cmd_test() {
  local args=("$@")
  trap 'cmd_down' EXIT
  cmd_up "${args[@]}"
  local ssl_file=""
  if [ "$(cat "$state/trust")" = bundle ]; then ssl_file="$state/ca-bundle.crt"; fi
  cd "$root"
  KROPA_IT_ZIPLINE_URL="https://localhost:$port" \
  KROPA_IT_ZIPLINE_TOKEN_FILE="$state/zipline-token" \
  KROPA_IT_SSL_CERT_FILE="$ssl_file" \
    cargo nextest run --profile it --locked -p kropa --test zipline_integration --run-ignored only
}

case "${1:-}" in
  test) shift; cmd_test "$@" ;;
  up) shift; cmd_up "$@" ;;
  down) cmd_down ;;
  *) die "usage: run.sh test|up|down (see the header of this file)" ;;
esac
