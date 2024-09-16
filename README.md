autost
======

**warning: this is currently a prototype. do not expect robustness or even usefulness.**

what if you could have a single-user self-hosted thing with the same posting and reading ux as cohost? what if you could then follow people and get a chronological timeline of their posts? what if you could share their posts too?

- **phase 1: convert chosts (we are here!)**
- phase 2: compose new posts
- phase 3: follow others

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
