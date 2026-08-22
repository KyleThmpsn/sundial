#!/bin/sh
set -eu

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_dir"

version=${1:-}
if [ -z "$version" ]; then
    version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
fi
version=${version#v}

target_dir=${CARGO_TARGET_DIR:-"$repo_dir/target"}
binary=${SUNDIAL_BINARY:-"$target_dir/release/sundial"}
dist_dir=${SUNDIAL_DIST_DIR:-"$repo_dir/dist"}
bundle_name="Sundial-v$version-linux-x86_64"
app_id="io.github.kylethmpsn.Sundial"

for required in \
    "$binary" \
    THIRD_PARTY_NOTICES.md \
    "assets/linux/$app_id.desktop" \
    "assets/linux/$app_id.png"; do
    if [ ! -f "$required" ]; then
        printf 'Missing required release file: %s\n' "$required" >&2
        exit 1
    fi
done

stage_root=$(mktemp -d)
trap 'rm -rf -- "$stage_root"' EXIT HUP INT TERM
bundle_dir="$stage_root/$bundle_name"
mkdir -p "$bundle_dir" "$dist_dir"

install -m755 "$binary" "$bundle_dir/sundial"
install -m755 packaging/linux/install.sh "$bundle_dir/install.sh"
install -m644 "assets/linux/$app_id.desktop" "$bundle_dir/$app_id.desktop"
install -m644 "assets/linux/$app_id.png" "$bundle_dir/$app_id.png"
install -m644 LICENSE README.md THIRD_PARTY_NOTICES.md "$bundle_dir/"

archive="$dist_dir/$bundle_name.tar.gz"
tar -C "$stage_root" -czf "$archive" "$bundle_name"
install -m755 "$binary" "$dist_dir/sundial"

(
    cd "$dist_dir"
    sha256sum "$bundle_name.tar.gz" > "$bundle_name.tar.gz.sha256"
    sha256sum sundial > sundial.sha256
)

printf 'Created %s\n' "$archive"
printf 'Created %s\n' "$dist_dir/sundial"
