#!/bin/sh
set -eu

app_id="io.github.kylethmpsn.Sundial"
bundle_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
bin_dir=${SUNDIAL_BIN_DIR:-"$HOME/.local/bin"}
data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
applications_dir="$data_home/applications"
icons_dir="$data_home/icons/hicolor/256x256/apps"

install -Dm755 "$bundle_dir/sundial" "$bin_dir/sundial"
install -Dm644 "$bundle_dir/$app_id.png" "$icons_dir/$app_id.png"

escaped_executable=$(printf '%s' "$bin_dir/sundial" | sed 's/\\/\\\\/g; s/"/\\"/g; s/`/\\`/g; s/\$/\\$/g')
mkdir -p "$applications_dir"
sed "s|^Exec=.*|Exec=\"$escaped_executable\"|" \
    "$bundle_dir/$app_id.desktop" > "$applications_dir/$app_id.desktop"
chmod 644 "$applications_dir/$app_id.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$applications_dir" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$data_home/icons/hicolor" >/dev/null 2>&1 || true
fi

printf 'Installed Sundial to %s\n' "$bin_dir/sundial"
printf 'The application launcher may take a moment to appear.\n'
