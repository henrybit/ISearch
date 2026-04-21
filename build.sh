#!/bin/bash
set -e

# ISearch Multi-Platform Build Script
# Usage: ./build.sh [platform]
#   platform: all (default), windows, mac, linux
#
# Note: Cross-compilation (building Windows/Linux on macOS) requires additional setup.
# On macOS, you can only build macOS binaries natively.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

PLATFORM="${1:-all}"

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_tauri() {
    if ! command -v npm &> /dev/null; then
        log_error "npm is not installed"
        exit 1
    fi

    if [ ! -d "src-tauri" ]; then
        log_error "src-tauri directory not found. Are you in the right directory?"
        exit 1
    fi
}

check_and_install_target() {
    local target=$1
    if ! rustup target list | grep -q "$target.*installed"; then
        log_info "Installing Rust target: $target"
        rustup target add "$target"
    fi
}

build_frontend() {
    log_info "Building frontend..."
    npm run build
}

build_windows() {
    log_info "Building for Windows..."
    check_and_install_target "x86_64-pc-windows-msvc"

    # Windows builds on non-Windows require cross toolchain
    if [ "$(uname)" != "Darwin" ] && [ "$(uname)" != "MINGW" ]; then
        log_warn "Cross-compilation for Windows requires cross toolchain"
        log_info "On Linux/macOS, install cross: cargo install cross"
        log_info "Or use GitHub Actions for Windows builds"
    fi

    npm run tauri build -- --target x86_64-pc-windows-msvc 2>&1 || {
        log_warn "Windows build failed - cross-compilation may not be available on this platform"
        return 1
    }
    log_info "Windows build complete: src-tauri/target/x86_64-pc-windows-msvc/release/isearch.exe"
}

build_mac() {
    log_info "Building for macOS..."

    # Build for current architecture
    local arch=$(uname -m)
    if [ "$arch" = "arm64" ]; then
        check_and_install_target "aarch64-apple-darwin"
        npm run tauri build -- --target aarch64-apple-darwin 2>&1 || {
            log_error "Apple Silicon build failed"
            return 1
        }
        log_info "macOS Apple Silicon build complete"
    else
        check_and_install_target "x86_64-apple-darwin"
        npm run tauri build -- --target x86_64-apple-darwin 2>&1 || {
            log_error "Intel macOS build failed"
            return 1
        }
        log_info "macOS Intel build complete"
    fi

    log_info "macOS build complete: src-tauri/target/"
}

build_linux() {
    log_info "Building for Linux..."

    if [ "$(uname)" = "Darwin" ]; then
        log_warn "Cross-compilation for Linux on macOS requires cross toolchain"
        log_info "Install cross: cargo install cross"
        log_info "Or use GitHub Actions for Linux builds"
    fi

    check_and_install_target "x86_64-unknown-linux-gnu"
    npm run tauri build -- --target x86_64-unknown-linux-gnu 2>&1 || {
        log_warn "Linux build failed - cross-compilation may not be available"
        return 1
    }
    log_info "Linux build complete: src-tauri/target/x86_64-unknown-linux-gnu/release/isearch"
}

create_bundles() {
    log_info "Creating bundle directory..."
    BUNDLE_DIR="$SCRIPT_DIR/bundles"
    mkdir -p "$BUNDLE_DIR"

    # Copy executables
    find src-tauri/target -name "isearch*" -type f -executable 2>/dev/null | while read -r f; do
        if [[ "$f" == *"release"* ]] && [[ ! "$f" == *".rlib"* ]]; then
            cp "$f" "$BUNDLE_DIR/" 2>/dev/null || true
        fi
    done

    # Copy installers
    find src-tauri/target -name "*.dmg" -type f 2>/dev/null | while read -r f; do
        cp "$f" "$BUNDLE_DIR/" 2>/dev/null || true
    done
    find src-tauri/target -name "*.msi" -type f 2>/dev/null | while read -r f; do
        cp "$f" "$BUNDLE_DIR/" 2>/dev/null || true
    done
    find src-tauri/target -name "*.AppImage" -type f 2>/dev/null | while read -r f; do
        cp "$f" "$BUNDLE_DIR/" 2>/dev/null || true
    done

    log_info "Bundles in: $BUNDLE_DIR"
    ls -la "$BUNDLE_DIR/" 2>/dev/null || true
}

main() {
    log_info "ISearch Multi-Platform Build Script"
    log_info "Current platform: $(uname) $(uname -m)"
    log_info "Building for: $PLATFORM"
    echo ""

    check_tauri

    case "$PLATFORM" in
        windows)
            build_frontend
            build_windows
            ;;
        mac)
            build_frontend
            build_mac
            ;;
        linux)
            build_frontend
            build_linux
            ;;
        all)
            build_frontend
            # On macOS, only macOS builds work natively
            if [ "$(uname)" = "Darwin" ]; then
                log_info "On macOS, only native macOS build is available"
                build_mac
                log_warn "Windows/Linux cross-compilation requires Linux CI or cross toolchain"
            elif [ "$(uname)" = "Linux" ]; then
                build_linux
                build_windows || log_warn "Windows build failed"
            else
                build_windows
                build_linux
            fi
            create_bundles
            ;;
        *)
            log_error "Unknown platform: $PLATFORM"
            echo "Usage: $0 [platform]"
            echo "  platform: all (default), windows, mac, linux"
            exit 1
            ;;
    esac

    echo ""
    log_info "Build completed!"
}

main
