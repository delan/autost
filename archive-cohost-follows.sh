#!/usr/bin/env zsh
# usage: ./archive-cohost-follows.sh <path/to/sites> <follows.txt>
# this generates follows.txt using the api if it doesn’t exist.
set -euo pipefail -o bsdecho
sites_path=$1; shift
follows_path=$1; shift

if [ -z "${COHOST_COOKIE+set}" ]; then
    >&2 echo 'COHOST_COOKIE not set!'
    exit 1
fi

if ! [ -e "$follows_path" ]; then
    >&2 echo '[*] getting list of followed projects'
    temp_path=$(mktemp)
    > "$temp_path" curl -b "connect.sid=$COHOST_COOKIE" 'https://cohost.org/api/v1/trpc/projects.followedFeed.query?input=%7B%22sortOrder%22:%22followed-asc%22,%22limit%22:1000,%22beforeTimestamp%22:1735199148430%7D'
    mv "$temp_path" "$follows_path"
fi

# if this fails, there were too many follows
< "$follows_path" jq -e '.result.data.nextCursor == null'

< "$follows_path" jq -er '.result.data.projects[].project.handle' | sort | while read -r project; do
    >&2 echo "[*] archiving: $project"
    ./archive-cohost-project.sh "$sites_path" "$project"
done
