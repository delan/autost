#!/usr/bin/env zsh
# usage: ./archive-cohost-project.sh <path/to/sites> <@project>
set -euo pipefail -o bsdecho
script_dir=${0:a:h}
sites_path=$1; shift
project=$1; shift

# accept both “@project” and “project” but strip the leading “@”
case "$project" in
(@*) project=${project#@} ;;
(*) project=$project ;;
esac

if [ -z "${COHOST_COOKIE+set}" ]; then
    >&2 echo 'COHOST_COOKIE not set!'
    exit 1
fi

cargo build -r
mkdir -p -- "$sites_path/$project"
cd -- "$sites_path/$project"

> autost.toml echo 'base_url = "/"'
>> autost.toml echo 'external_base_url = "https://example.com/"'
>> autost.toml echo 'site_title = "@'"$project"'"'
>> autost.toml echo 'other_self_authors = ["https://cohost.org/'"$project"'"]'
>> autost.toml echo 'interesting_tags = []'
>> autost.toml echo '[[nav]]'
>> autost.toml echo 'href = "."'
>> autost.toml echo 'text = "posts"'
if ! [ -e cohost2json.done ]; then
    >&2 echo "[@$project] autost cohost2json $project chosts"
    "$script_dir/target/release/autost" cohost2json "$project" chosts
    touch cohost2json.done
fi
if ! [ -e cohost2autost.done ]; then
    >&2 echo "[@$project] autost cohost2autost chosts"
    "$script_dir/target/release/autost" cohost2autost chosts
    touch cohost2autost.done
fi
