#!/usr/bin/env bash
set -euo pipefail

ANDROID_DIR="$(cd "$(dirname "\${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "\${ANDROID_DIR}/.." && pwd)"
OUTPUT_DIR="\${ANDROID_DIR}/app/src/main/jniLibs"
TARGET="aarch64-linux-android"

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo is required to build the Rust JNI library" >&2
    exit 1
fi

if ! command -v rustup >/dev/null 2>&1; then
    echo "error: rustup is required to install the \${TARGET} target" >&2
    exit 1
fi

if [[ -z "\${ANDROID_NDK_HOME:-}" && -z "\${ANDROID_NDK_ROOT:-}" ]]; then
    echo "error: set ANDROID_NDK_HOME (or ANDROID_NDK_ROOT) to an installed Android NDK" >&2
    exit 1
fi

if ! command -v cargo-ndk >/dev/null 2>&1; then
    echo "cargo-ndk not found; installing it with cargo install --locked" >&2
    cargo install cargo-ndk --locked
fi

rustup target add "\${TARGET}"
mkdir -p "\${OUTPUT_DIR}"

export CARGO_TARGET_DIR="\${CARGO_TARGET_DIR:-\${REPO_ROOT}/target/android}"
cd "\${REPO_ROOT}"
cargo ndk \
    -t arm64-v8a \
    -o "\${OUTPUT_DIR}" \
    build \
    -p tg-android-bridge \
    --release

LIBRARY="\${OUTPUT_DIR}/arm64-v8a/libtg_android_bridge.so"
if [[ ! -f "\${LIBRARY}" ]]; then
    echo "error: expected JNI library was not produced at \${LIBRARY}" >&2
    exit 1
fi

echo "Rust JNI library: \${LIBRARY}"
