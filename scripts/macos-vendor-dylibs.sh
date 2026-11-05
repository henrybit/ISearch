#!/usr/bin/env bash
# Copy Homebrew's libclamav and its non-system dylib dependencies into src-tauri/macos-libs,
# rewriting install names to @rpath so they can ship in ISearch.app/Contents/Frameworks.
#
# Outputs:
#   src-tauri/macos-libs/*.dylib            vendored libraries (+ libclamav.dylib link symlink)
#   src-tauri/macos-libs/env                CLAMAV_LIBRARY / CLAMAV_INCLUDE / OPENSSL_INCLUDE for clamav-sys
#   src-tauri/tauri.macos-libs.conf.json    pass to `tauri build --config` to bundle the libraries
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/src-tauri/macos-libs"
CONF="$ROOT/src-tauri/tauri.macos-libs.conf.json"

CLAMAV_PREFIX="$(brew --prefix clamav)"
OPENSSL_PREFIX="$(brew --prefix openssl@3)"

rm -rf "$OUT"
mkdir -p "$OUT"

install_id() {
	otool -D "$1" | tail -n +2
}

is_system() {
	case "$1" in
		/usr/lib/* | /System/*) return 0 ;;
		*) return 1 ;;
	esac
}

# Resolve a dependency reference (as listed by otool -L) relative to the referencing library.
resolve_dep() {
	local ref="$1" from_dir="$2"
	case "$ref" in
		@rpath/* | @loader_path/*) realpath "$from_dir/${ref#*/}" ;;
		*) realpath "$ref" ;;
	esac
}

clamav_src="$(realpath "$CLAMAV_PREFIX/lib/libclamav.dylib")"
clamav_name="$(basename "$(install_id "$clamav_src")")"

seen=" "
names=()
queue=("$clamav_src")
while [ "${#queue[@]}" -gt 0 ]; do
	src="${queue[0]}"
	queue=("${queue[@]:1}")

	self_id="$(install_id "$src")"
	name="$(basename "$self_id")"
	case "$seen" in *" $name "*) continue ;; esac
	seen="$seen$name "
	names+=("$name")

	dest="$OUT/$name"
	cp "$src" "$dest"
	chmod u+w "$dest"
	install_name_tool -id "@rpath/$name" "$dest" 2>/dev/null
	# Drop build-time rpaths (e.g. Homebrew Cellar) so @rpath only resolves inside the .app.
	for rp in $(otool -l "$dest" | awk '/cmd LC_RPATH/ {getline; getline; print $2}'); do
		install_name_tool -delete_rpath "$rp" "$dest" 2>/dev/null
	done

	for ref in $(otool -L "$src" | tail -n +2 | awk '{print $1}'); do
		[ "$ref" = "$self_id" ] && continue
		is_system "$ref" && continue
		dep="$(resolve_dep "$ref" "$(dirname "$src")")"
		install_name_tool -change "$ref" "@rpath/$(basename "$(install_id "$dep")")" "$dest" 2>/dev/null
		queue+=("$dep")
	done
done

# install_name_tool invalidates signatures; arm64 refuses to load unsigned code, so ad-hoc sign.
# Tauri re-signs everything in Contents/Frameworks with the real identity when bundling.
for name in "${names[@]}"; do
	codesign --force --sign - "$OUT/$name" 2>/dev/null
done

ln -s "$clamav_name" "$OUT/libclamav.dylib"

{
	echo "CLAMAV_LIBRARY=$OUT/libclamav.dylib"
	echo "CLAMAV_INCLUDE=$CLAMAV_PREFIX/include"
	echo "OPENSSL_INCLUDE=$OPENSSL_PREFIX/include"
} > "$OUT/env"

{
	printf '{\n  "bundle": {\n    "macOS": {\n      "frameworks": [\n'
	for i in "${!names[@]}"; do
		sep=","
		[ "$i" -eq $((${#names[@]} - 1)) ] && sep=""
		printf '        "./macos-libs/%s"%s\n' "${names[$i]}" "$sep"
	done
	printf '      ]\n    }\n  }\n}\n'
} > "$CONF"

echo "[macos-libs] Vendored into $OUT:"
for name in "${names[@]}"; do
	echo "  $name"
done
