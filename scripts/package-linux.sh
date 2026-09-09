#!/bin/sh
set -eu

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_dir"

version=${1:-}
if [ -z "$version" ]; then
    version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1 | tr -d '\r')
fi
version=${version#v}
case $version in
    "" | *[!0-9A-Za-z.+-]*)
        printf 'Release version contains unsafe filename characters: %s\n' "$version" >&2
        exit 1
        ;;
esac

target_dir=${CARGO_TARGET_DIR:-"$repo_dir/target"}
sundial_binary=${SUNDIAL_BINARY:-"$target_dir/release/sundial"}
dist_dir=${SUNDIAL_DIST_DIR:-"$repo_dir/dist"}
bundle_name="Sundial-v$version-linux-x86_64"
app_id="io.github.kylethmpsn.Sundial"

for required in \
    "$sundial_binary" \
    LICENSE \
    README.md \
    assets/sundial-alt.png \
    crates/parhelion/README.md \
    packaging/THIRD_PARTY_NOTICES.txt \
    packaging/linux/install.sh \
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
mkdir -p "$bundle_dir/crates/parhelion" "$bundle_dir/assets" "$dist_dir"

install -m755 "$sundial_binary" "$bundle_dir/sundial"
install -m755 packaging/linux/install.sh "$bundle_dir/install.sh"
install -m644 "assets/linux/$app_id.desktop" "$bundle_dir/$app_id.desktop"
install -m644 "assets/linux/$app_id.png" "$bundle_dir/$app_id.png"
install -m644 LICENSE README.md "$bundle_dir/"
install -m644 assets/sundial-alt.png "$bundle_dir/assets/sundial-alt.png"
install -m644 crates/parhelion/README.md "$bundle_dir/crates/parhelion/README.md"
install -m644 packaging/THIRD_PARTY_NOTICES.txt "$bundle_dir/THIRD_PARTY_NOTICES.txt"

archive="$dist_dir/$bundle_name.tar.gz"
tar -C "$stage_root" -czf "$archive" "$bundle_name"

expected="$stage_root/expected.txt"
actual="$stage_root/actual.txt"
printf '%s\n' \
    "$bundle_name/sundial" \
    "$bundle_name/install.sh" \
    "$bundle_name/$app_id.desktop" \
    "$bundle_name/$app_id.png" \
    "$bundle_name/LICENSE" \
    "$bundle_name/README.md" \
    "$bundle_name/assets/sundial-alt.png" \
    "$bundle_name/crates/parhelion/README.md" \
    "$bundle_name/THIRD_PARTY_NOTICES.txt" \
    | LC_ALL=C sort > "$expected"
tar -tzf "$archive" | sed '/\/$/d' | LC_ALL=C sort -u > "$actual"
if ! cmp -s "$expected" "$actual"; then
    printf 'Archive file allowlist mismatch: %s\n' "$archive" >&2
    diff -u "$expected" "$actual" >&2 || true
    exit 1
fi

(
    cd "$dist_dir"
    sha256sum "$bundle_name.tar.gz" > "$bundle_name.tar.gz.sha256"
)

printf 'Created %s\n' "$archive"
