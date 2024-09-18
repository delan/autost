autost
======

**warning: this is currently a prototype. do not expect robustness or even usefulness.**

what if you could have a single-user self-hosted thing with the same posting and reading ux as cohost? what if you could then follow people and get a chronological timeline of their posts? what if you could share their posts too?

1. **archive chosts (we are here!)**
    - [x] download chosts from the api (`cohost2json`)
    - [x] extract and render chost content (`cohost2autost`)
        - [x] extract cohost-rendered chost content
        - [x] render asks
        - [x] render image attachments
        - [ ] render audio attachments
        - [ ] render attachment rows (new post editor)
    - [x] generate the main page (`index.html`)
    - [x] generate chost pages (`<postId>.html`)
    - [x] generate tag pages (`tagged/<tag>.html`)
2. curate chosts
    - [x] select tags to include on the main page (`interesting_tags`)
    - [x] select posts to include on the main page (`interesting_archived_threads_list_path`)
    - [x] select posts to exclude from the main page (`excluded_archived_threads_list_path`)
    - [x] deploy only included posts, to avoid enumeration (`interesting_output_filenames_list_path`)
    - [x] generate pages for all posts, posts not yet interesting/excluded, …
    - [ ] add tags to chosts without editing the originals
3. compose new posts
4. follow others
    - [x] generate atom feeds (`index.feed.xml`, `tagged/<tag>.feed.xml`)

## how to dump your own chosts

```
$ mkdir -p path/to/chosts
$ read -r COHOST_COOKIE; export COHOST_COOKIE  # optional
$ cargo run --bin cohost2json -- projectName path/to/chosts
```

## how to convert chosts to autosts

```
$ mkdir -p path/to/autosts site/attachments
$ cargo run --bin cohost2autost -- path/to/chosts path/to/autosts site/attachments
```

or to convert specific chosts only:

```
$ cargo run --bin cohost2autost -- path/to/chosts path/to/autosts site/attachments 123456.json 234567.json
```

## how to render your autosts to pages

```
$ cargo run -- site path/to/autosts/*
```
