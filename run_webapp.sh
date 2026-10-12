#!/usr/bin/env bash
# Builds and runs LightPost on Linux and macOS. Mirrors run_webapp.ps1.
#
#   ./run_webapp.sh                     # debug build on http://127.0.0.1:3000
#   ./run_webapp.sh --release --port 3100 --token "my-secret-token"
#   ./run_webapp.sh --dry-run
set -euo pipefail

cd "$(dirname "$0")"

port=3000
bind_host=127.0.0.1
base_url=""
token="local-dev-secret-token"
data_dir="$PWD/data"
profile=debug
build=1
dry_run=0

usage() {
  sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//'
  echo "options: --port N --host IP --base-url URL --token TOKEN --data-dir DIR --release --no-build --dry-run"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --port) port="$2"; shift 2 ;;
    --host) bind_host="$2"; shift 2 ;;
    --base-url) base_url="$2"; shift 2 ;;
    --token) token="$2"; shift 2 ;;
    --data-dir) data_dir="$2"; shift 2 ;;
    --release) profile=release; shift ;;
    --no-build) build=0; shift ;;
    --dry-run) dry_run=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

command -v cargo >/dev/null || { echo "cargo not found; install the Rust toolchain first" >&2; exit 1; }
mkdir -p "$data_dir"

echo "LightPost"
echo "  Root:    $PWD"
echo "  Address: http://$bind_host:$port"
echo "  Data:    $data_dir"
echo "  Build:   $profile"
echo "  Admin:   http://$bind_host:$port/admin"

if [ "$dry_run" -eq 1 ]; then
  echo "Dry run: the server was not started."
  exit 0
fi

if [ "$build" -eq 1 ]; then
  if [ "$profile" = release ]; then cargo build --release; else cargo build; fi
fi

export LIGHTPOST_HOST="$bind_host"
export LIGHTPOST_PORT="$port"
export LIGHTPOST_BASE_URL="$base_url"
export LIGHTPOST_ADMIN_TOKEN="$token"
export LIGHTPOST_DATA_DIR="$data_dir"
export RUST_BACKTRACE=1
exec "target/$profile/lightpost"
