#!/bin/sh
# usage: deploy.sh <host:/path/to/site> <interesting_output_filenames_list_path> [-n]
# -n = dry run
set -eu
dest=$1; shift
interesting_output_filenames_list_path=$1; shift
set -x
upload() {
    # `--relative` means source paths like tagged/foo.feed.xml create a `tagged`
    # directory on the destination, without flattening the directory structure.
    rsync -av --no-i-r --info=progress2 --relative "$@" "$dest"
}
# `/./` means `--relative` only includes the part to the right, so the `site`
# part still gets flattened on the destination. we do this instead of `cd site`
# because the `$interesting_output_filenames_list_path` may be relative.
upload "$@" site/./attachments site/./*.css site/./*.js site/./*.woff2 site/./*.pdf
upload "$@" --files-from="$interesting_output_filenames_list_path" site/./
