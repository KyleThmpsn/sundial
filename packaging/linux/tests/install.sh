#!/bin/sh
set -eu

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
test_root=$(mktemp -d)
trap 'rm -rf -- "$test_root"' EXIT HUP INT TERM
bundle="$test_root/bundle"
mkdir -p "$bundle"
cp "$repo_dir/packaging/linux/install.sh" "$bundle/install.sh"
cp "$repo_dir/assets/linux/io.github.kylethmpsn.Sundial.png" "$bundle/"
tr -d '\r' < "$repo_dir/assets/linux/io.github.kylethmpsn.Sundial.desktop" \
    > "$bundle/io.github.kylethmpsn.Sundial.desktop"
cat > "$bundle/sundial" <<'EOF'
#!/bin/sh
printf '%s\n' "$0" > "$SUNDIAL_INSTALL_TEST_RESULT"
EOF

for component in 'ordinary path' 'quote"path' 'dollar$path' 'tick`path' 'percent%fpath' 'double%%path' 'back\slash'; do
    export SUNDIAL_BIN_DIR="$test_root/$component/bin"
    export XDG_DATA_HOME="$test_root/$component/data"
    export SUNDIAL_INSTALL_TEST_RESULT="$test_root/$component/launched.txt"
    sh "$bundle/install.sh" > /dev/null
    cmp "$bundle/sundial" "$SUNDIAL_BIN_DIR/sundial"
    desktop="$XDG_DATA_HOME/applications/io.github.kylethmpsn.Sundial.desktop"
    desktop-file-validate "$desktop"
    /usr/bin/python3 - "$desktop" "$SUNDIAL_BIN_DIR/sundial" "$SUNDIAL_INSTALL_TEST_RESULT" <<'PY'
import pathlib
import sys
import time

from gi.repository import Gio

desktop, executable, marker = sys.argv[1:]
app = Gio.DesktopAppInfo.new_from_filename(desktop)
assert app is not None, desktop
assert app.launch([], None), desktop
result = pathlib.Path(marker)
for _ in range(250):
    if result.exists() and result.read_text() == executable + "\n":
        break
    time.sleep(0.02)
else:
    raise AssertionError(f"Launcher did not execute {executable!r}")
PY
done

# A link already at the launcher's path is replaced, never followed: the file it names keeps
# its bytes and mode, a dangling link's target is not created, and a linked folder stays empty.
for link in file dangling folder; do
    export SUNDIAL_BIN_DIR="$test_root/link-$link/bin"
    export XDG_DATA_HOME="$test_root/link-$link/data"
    applications="$XDG_DATA_HOME/applications"
    desktop="$applications/io.github.kylethmpsn.Sundial.desktop"
    sentinel="$test_root/link-$link/sentinel"
    mkdir -p "$applications"
    case $link in
        file)
            printf 'KEEP MY CONTENTS\n' > "$sentinel"
            chmod 600 "$sentinel"
            ;;
        folder) mkdir -p "$sentinel" ;;
        dangling) ;;
    esac
    ln -s "$sentinel" "$desktop"
    sh "$bundle/install.sh" > /dev/null
    [ -f "$desktop" ] && [ ! -L "$desktop" ]
    desktop-file-validate "$desktop"
    case $link in
        file)
            [ "$(cat "$sentinel")" = 'KEEP MY CONTENTS' ]
            [ "$(stat -c %a "$sentinel")" = 600 ]
            ;;
        folder) [ -z "$(ls -A "$sentinel")" ] ;;
        dangling) [ ! -e "$sentinel" ] ;;
    esac
    [ -z "$(find "$applications" -name '.io.github.kylethmpsn.Sundial.*')" ]
done
printf 'Linux desktop installation and launch passed for seven path cases and three linked launchers.\n'
