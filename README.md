autost
======

**questions and contributions welcome :3**

want to **archive your chosts on your website** but have too many for the [cohost web component](https://cohost.org/astral/post/7796845-div-style-position)? want something like [cohost-dl](https://cohost.org/blep/post/7639936-cohost-dl) except **you can keep posting**? what if your blog engine had the same posting *and reading* experience as cohost? what if you could follow people with rss/atom feeds and see their posts on a chronological timeline? what if you could share their posts too?

1. archive your chosts
    - [x] download chosts from the api (`cohost2json`)
    - [x] import chosts from the api (`cohost2autost`)
    - [ ] import chosts from [cohost-dl](https://cohost.org/blep/post/7639936-cohost-dl)
    - [ ] import chosts from your cohost data export
    - [x] extract and render chost content
        - [x] download and rewrite cohost cdn links
        - [x] extract cohost-rendered chost content
        - [x] render asks
        - [x] render image attachments
        - [x] render audio attachments
        - [x] render attachment rows (new post editor)
    - [x] generate the main page (`index.html`)
    - [x] generate chost pages (`<postId>.html`)
    - [x] generate tag pages (`tagged/<tag>.html`)
2. curate your chosts
    - [x] select tags to include on the main page (`interesting_tags`)
    - [x] select posts to include on the main page (`interesting_archived_threads_list_path`)
    - [x] select posts to exclude from the main page (`excluded_archived_threads_list_path`)
    - [x] deploy only included posts, to avoid enumeration (`interesting_output_filenames_list_path`)
    - [x] generate pages for all posts, posts not yet interesting/excluded, …
    - [x] add tags to chosts without editing the originals (`archived_thread_tags_path`)
    - [x] automatically rename tags whenever encountered (tag synonyms; `renamed_tags`)
    - [x] add tags whenever a tag is encountered (tag implications; `implied_tags`)
3. **compose new posts (we are here!)**
    - [x] compose simple posts
    - [x] compose replies
    - [ ] upload attachments
4. follow others
    - [x] generate atom feeds (`index.feed.xml`, `tagged/<tag>.feed.xml`)
    - [ ] subscribe to feeds
    - [ ] single reverse chronological timeline
    - [ ] share and reply to posts

## getting started

**go to [the releases page](https://github.com/delan/autost/releases) to download or install autost!**

it’s a single program you run in your terminal (`autost`).

## make a new site

```
$ autost new sites/example.com  # example (can be anywhere)
$ cd sites/example.com
```

## how to dump your own chosts

cohost “projects” are the things with handles like `@staff` that you can have more than one of.

```
$ cd sites/example.com
$ autost cohost2json projectName path/to/chosts
```

you may want to dump private or logged-in-only chosts, be they your own or those of people you’ve followed or reblogged. in this case, you will need to set COHOST_COOKIE to the value of your “connect.sid” cookie as follows, **and switch projects in the cohost web ui**, otherwise you won’t see everything!

```
$ read -r COHOST_COOKIE; export COHOST_COOKIE  # optional
```

## how to convert chosts to posts

```
$ cd sites/example.com
$ autost cohost2autost path/to/chosts
```

or to convert specific chosts only:

```
$ cd sites/example.com
$ autost cohost2autost path/to/chosts 123456.json 234567.json
```

## how to render your posts to pages

```
$ cd sites/example.com
$ autost render
```

or to render specific posts only:

```
$ cd sites/example.com
$ autost render posts/123456.html posts/10000000.md
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

## how to start the server so you can post

**warning: this server has no password and no sandboxing yet! do not expose it to the internet!**

```
$ cd sites/example.com
$ autost server
```

## how to deploy

the best way to upload your site to a web host depends on if you have chosts you might not want people to see. if you upload everything, someone can count from 1.html to 9999999.html and find all of your chosts.

if you want to upload everything, you can use rsync directly (note the trailing slash):

```
$ cd sites/example.com
$ rsync -av site/ host:/var/www/example.com
```

if you want to only upload the chosts you have curated, you can use deploy.sh (where path/to/interesting.txt is your `interesting_output_filenames_list_path`):

```
$ cd sites/example.com
$ ../../deploy.sh host:/var/www/example.com path/to/interesting.txt -n  # dry run
$ ../../deploy.sh host:/var/www/example.com path/to/interesting.txt     # wet run
```

## suggested workflow

if you just want to back up your chosts, make an autost site for each cohost project, like `sites/@catball` and `sites/@rats`.

if you want to do anything more involved, you should make a `staging` and `production` version of your autost site, like `sites/staging` and `sites/production`:

- to render your site, `cd sites/staging; autost render`
- to see what changed, `colordiff -ru sites/production sites/staging`
- if you’re happy with the changes, `rsync -a sites/staging sites/production`
- and finally to deploy, `cd sites/production` and see “how to deploy”

that way, you can catch unintentional changes or autost bugs, and you have a backup of your site in case anything goes wrong.

## troubleshooting

if something goes wrong, you can set RUST_LOG or RUST_BACKTRACE to get more details:

```
$ export RUST_LOG=autost=debug
$ export RUST_LOG=autost=trace
$ export RUST_BACKTRACE=1
```

## building autost yourself

if you want to tinker with autost, [install rust](https://rustup.rs), then download and build the source (see below). replace `autost` in the commands above with `cargo run -r --`.

```
$ git clone https://github.com/delan/autost.git
$ cd autost
$ cargo run -r -- server
```
