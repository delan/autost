autost
======

what if you could have a single-user self-hosted thing with the same posting and reading ux as cohost? what if you could then follow people and get a chronological timeline of their posts? what if you could share their posts too?

## how to dump your own chosts

```
$ mkdir -p path/to/chosts
$ cargo run --bin cohost2json -- projectName path/to/chosts
```

## how to convert chosts to autosts

```
$ mkdir -p path/to/autosts path/to/attachments
$ cargo run --bin cohost2autost -- path/to/chosts path/to/autosts path/to/attachments
```

## how to render your autosts to a page

```
$ cargo run -- path/to/autosts/* > autosts.html
```
