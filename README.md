autost
======

**warning: this is currently a prototype. do not expect robustness or even usefulness.**

what if you could have a single-user self-hosted thing with the same posting and reading ux as cohost? what if you could then follow people and get a chronological timeline of their posts? what if you could share their posts too?

1. **archive chosts (we are here!)**
    - [x] download chosts from the api (`cohost2json`)
    - [x] extract and render chost content (`cohost2autost`)
        - [x] download and rewrite cohost cdn links
        - [x] extract cohost-rendered chost content
        - [x] render asks
        - [x] render image attachments
        - [ ] render audio attachments
        - [x] render attachment rows (new post editor)
    - [x] generate the main page (`index.html`)
    - [x] generate chost pages (`<postId>.html`)
    - [x] generate tag pages (`tagged/<tag>.html`)
2. curate chosts
    - [x] select tags to include on the main page (`interesting_tags`)
    - [x] select posts to include on the main page (`interesting_archived_threads_list_path`)
    - [x] select posts to exclude from the main page (`excluded_archived_threads_list_path`)
    - [x] deploy only included posts, to avoid enumeration (`interesting_output_filenames_list_path`)
    - [x] generate pages for all posts, posts not yet interesting/excluded, …
    - [x] add tags to chosts without editing the originals (`archived_thread_tags_path`)
    - [x] automatically rename tags whenever encountered (tag synonyms; `renamed_tags`)
    - [x] add tags whenever a tag is encountered (tag implications; `implied_tags`)
3. compose new posts
4. follow others
    - [x] generate atom feeds (`index.feed.xml`, `tagged/<tag>.feed.xml`)

## how to dump your own chosts

```
$ mkdir -p path/to/chosts
$ read -r COHOST_COOKIE; export COHOST_COOKIE  # optional
$ RUST_LOG=info cargo run --bin cohost2json -- projectName path/to/chosts
```

## how to convert chosts to autosts

```
$ mkdir -p path/to/autosts site/attachments
$ RUST_LOG=info cargo run --bin cohost2autost -- path/to/chosts path/to/autosts site/attachments
```

or to convert specific chosts only:

```
$ RUST_LOG=info cargo run --bin cohost2autost -- path/to/chosts path/to/autosts site/attachments 123456.json 234567.json
```

## how to render your autosts to pages

```
$ RUST_LOG=info cargo run -- site path/to/autosts/*
```

## how to include or exclude specific chosts

1. set the `interesting_archived_threads_list_path` or `excluded_archived_threads_list_path` to a text file
2. in the text file, add a line for each chost with the original cohost url

## how to add tags to converted chosts

1. set the `archived_thread_tags_path` to a text file
2. in the text file, add a line for each chost as follows:

```
https://cohost.org/project/post/123456-slug tag,another tag
```
