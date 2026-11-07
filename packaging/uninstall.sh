#!/bin/sh
# Remove files installed by packaging/install.sh. Keeps user data.
#
#   packaging/uninstall.sh [--prefix DIR] [--yes] [--dry-run]
set -eu

PREFIX="${YUTU_INSTALL_PREFIX:-$HOME/.local}"
ASSUME_YES=0
DRY_RUN=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --prefix)
            [ "$#" -ge 2 ] || { echo "error: --prefix needs a directory" >&2; exit 1; }
            PREFIX=$2
            shift
            ;;
        --yes) ASSUME_YES=1 ;;
        --dry-run) DRY_RUN=1 ;;
        -h|--help)
            sed -n '2,4p' "$0"
            exit 0
            ;;
        *) echo "error: unknown option: $1" >&2; exit 1 ;;
    esac
    shift
done

case "$PREFIX" in
    /*) ;;
    *) echo "error: prefix must be absolute: $PREFIX" >&2; exit 1 ;;
esac
[ "$PREFIX" != "/" ] || { echo "error: refusing prefix /" >&2; exit 1; }
case "$PREFIX" in
    *"/.."|*"/../"*|"/.."|*"/./"*|*"/.") echo "error: prefix must not contain . or .. components" >&2; exit 1 ;;
esac

if [ "$ASSUME_YES" -eq 0 ] && [ "$DRY_RUN" -eq 0 ]; then
    [ -t 0 ] || { echo "error: rerun with --yes to uninstall" >&2; exit 1; }
    printf 'Remove yutu-mux files below %s? User data is kept. [y/N] ' "$PREFIX"
    IFS= read -r answer
    case "$answer" in
        y|Y|yes|YES|Yes) ;;
        *) echo "cancelled" >&2; exit 1 ;;
    esac
fi

remove_file() {
    if [ -e "$1" ] || [ -L "$1" ]; then
        if [ "$DRY_RUN" -eq 1 ]; then
            printf 'would remove %s\n' "$1"
        else
            rm -rf -- "$1"
            printf 'removed %s\n' "$1"
        fi
    fi
}

remove_file "$PREFIX/bin/yutu-mux"
remove_file "$PREFIX/share/applications/yutu-mux.desktop"
# Icon theme entries installed alongside the Linux launcher. A dangling
# glob matches nothing and remove_file skips missing paths.
# shellcheck disable=SC2086
for icon in "$PREFIX"/share/icons/hicolor/*/apps/yutu-mux.png \
    "$PREFIX/share/icons/hicolor/scalable/apps/yutu-mux.svg"; do
    remove_file "$icon"
done
remove_file "$PREFIX/Applications/YutuMux.app"
printf 'User library, cache, and login data were not removed.\n'
