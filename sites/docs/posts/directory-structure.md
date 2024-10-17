<meta name="title" content="directory structure">
<meta name="published" content="2024-10-01T06:00Z">
<link rel="author" href="#" name="autost">
<meta name="author_display_name" content="autost">
<meta name="author_display_handle" content="autost.example">

`autost.toml` is where your site settings go, and any directory that contains one is an autost site. create one with `autost new <path/to/site/directory>`.

`/posts/` (`PostsPath` internally) is where your post sources are stored, as `.md` or `.html` files. only files at the top level of this directory are considered when rendering your site, but files in subdirectories can still be replied to (`<link rel=references>`).
- `1.html` … `9999999.html` for chosts (`autost cohost2autost`)
  - `1/1.html` … `1/9999999.html` for chosts in the thread of chost id 1
- `10000000.md` or `10000000.html` and beyond for your other posts
- `imported/1.html` and beyond for other imported posts (`autost import`)

`/attachments/` (`AttachmentsPath` internally), is where your attachments are stored, including attachments cached from chosts or other imported posts.
- `<uuid>/<original filename>` for your own attachments and attachments in chosts
- `thumbs/<uuid>/<original filename>` for thumbnails of attachments in chosts
- `imported-<id>-<sha256 of url>/file.<ext>` for attachments in other imported posts
- `emoji/<id>/file.<ext>` for emoji in chosts

`/site/` (`SitePath` internally), or the *site output path*, is where your site gets rendered to. you can delete this directory whenever you want a clean build.
- `1.html` … `9999999.html` for each of your “interesting” chosts
- `10000000.html` and beyond for your other posts (always “interesting”)
- `index.html` and `index.feed.xml` for all of your “interesting” posts
- `tagged/<tag>.html` and `tagged/<tag>.feed.xml` for each “interesting” tag
- `attachments/` is a mirror of your `/attachments/` directory, using hard links
- plus several static files copied from the program binary or `path_to_static`
  - `deploy.sh` uses rsync to upload your “interesting” posts to a web server
