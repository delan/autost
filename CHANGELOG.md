# [1.3.1](https://github.com/delan/autost/releases/tag/1.3.1) (2024-12-30)

in `autost cohost2json` and `autost cohost-archive`...
- **fixed a bug causing incorrect output in `--liked` mode** ([#34](https://github.com/delan/autost/issues/34))
  - this only affects archives of your own cohost projects, not other people’s projects
  - if you used `autost cohost2json`, please rerun both `autost cohost2json` and `autost cohost2autost` to fix your archived chosts
  - if you used `autost cohost-archive`, please delete both `cohost2json.done` and `cohost2autost.done` on only the archived projects you need to fix, then rerun it

in `autost cohost2autost` and `autost cohost-archive`...
- no longer crashes when attachment filenames contain `:` on windows (@echowritescode, [#32](https://github.com/delan/autost/pull/32))

# [1.3.0](https://github.com/delan/autost/releases/tag/1.3.0) (2024-12-29)

in `autost cohost2json` and `autost cohost-archive`...
- **you can now include your own liked chosts** with `--liked` ([@Sorixelle](https://github.com/Sorixelle), [#31](https://github.com/delan/autost/pull/31))
  - liked chosts can be found at liked.html, e.g. <http://[::1]:8420/liked.html>
  - if you used `autost cohost-archive`, remember to delete both `cohost2json.done` and `cohost2autost.done` on archived projects you want to update

in `autost cohost2autost` and `autost cohost-archive`...
- now handles some malformed but technically valid attachment urls on staging.cohostcdn.org
- now handles attachment urls that 404 gracefully, by logging an error and continuing

# [1.2.1](https://github.com/delan/autost/releases/tag/1.2.1) (2024-12-28)

couple of small improvements to `autost cohost-archive`...
- **now archives your own chosts too**, then the people you follow, if no specific projects are given
- **now tells you which project is currently being archived**, in the log output

# [1.2.0](https://github.com/delan/autost/releases/tag/1.2.0) (2024-12-28)

- **be sure to rerun `autost cohost-archive` and/or `autost cohost2autost` before cohost shuts down!** why?
  - **we now archive cohost attachments in inline styles**, like `<div style="background: url(cohostcdn...)">`
  - **we now archive hotlinked cohost emotes, eggbug logos, etc**, like <https://cohost.org/static/f0c56e99113f1a0731b4.svg>
  - **we now archive hotlinked cohost avatar and header images**, like in <https://cohost.org/srxl/post/4940861-p-style-padding-to>
  - attachments are fetched and rewritten when chosts (json) are converted to posts, so you will need to reconvert your chosts
  - to make `autost cohost-archive` actually reconvert chosts, delete the `cohost2autost.done` file in each archived project
- **got nix?**
  - you can now run the latest *released* version of autost with `nix run github:delan/autost/latest`
  - you can now get prebuilt binaries [on cachix](https://autost.cachix.org); see the README for details ([@Sorixelle](https://github.com/Sorixelle), [#30](https://github.com/delan/autost/pull/30))

in `autost cohost-archive`...
- archived chosts are now visible on the main page, without needing to navigate to <http://[::1]:8420/all.html>
  - **you can rerun `autost cohost-archive` to update your existing archives!** it’s smart enough to not need to redownload and reconvert the chosts, but see above for why you should reconvert anyway

in `autost server`...
- **you can now override the listening port** with `-p` (`--port`)

posts by the `[self_author] href` are always considered “interesting”, but the check for this has been improved:
- fixed a bug that prevented archived chosts with that `href` from being considered “interesting”
- fixed a bug that prevented you from changing your `name`, `display_name`, or `handle`

as a result of these changes...
- you can now publish all of your archived chosts at once by setting your `href` to the url of your cohost page (e.g. <https://cohost.org/staff>)
- you can now change your `name`, `display_name`, or `handle` without editing all of your old posts

# [1.1.0](https://github.com/delan/autost/releases/tag/1.1.0) (2024-12-27)

- **be sure to rerun `autost cohost2autost` before cohost shuts down!** why?
  - we’ve fixed a bug that broke audio attachments in a variety of places, including `autost render`, `autost server`, `autost import`, and `autost cohost2autost`
- **got nix?** you can now run autost *without any extra setup* with `nix run github:delan/autost`, or build autost with `nix build`, `nix develop`, or `nix-shell` ([@Sorixelle](https://github.com/Sorixelle), [#25](https://github.com/delan/autost/pull/25))
- you can now easily **archive the chosts of everyone you follow** (`autost cohost-archive`)

in the html output...

- **posts can now contain &lt;video> and &lt;details name>**
- **posts now have [link previews](https://ogp.me)**, with title, description, and the first image if any ([#21](https://github.com/delan/autost/issues/21))
- **&lt;pre> elements** in the top level of a post **are now scrollable**, if their contents are too wide
- post titles are now marked as the h-entry title (.p-name), making them more accurately rebloggable
- posts without titles now have a placeholder title, like “untitled post by @staff”
- authors without display names are handled better ([#23](https://github.com/delan/autost/issues/23))

in `autost cohost2json`...
- no longer hangs after fetching ~120 pages of posts ([#15](https://github.com/delan/autost/issues/15))
- no longer crashes in debug builds ([#15](https://github.com/delan/autost/issues/15))

in `autost cohost2autost`...
- **cohost emotes** like `:eggbug:` are now converted
- **authors without display names** are handled better ([#23](https://github.com/delan/autost/issues/23))
- now handles posts with deeply nested dom trees without crashing ([#28](https://github.com/delan/autost/issues/28))
- now retries attachment redirect requests, since they occasionally fail ([#29](https://github.com/delan/autost/issues/29))
- now runs faster, by walking the dom tree only once per post

in `autost import`...
- h-entry author names (.p-author .p-name), and other p-properties, can now be extracted from &lt;abbr>, &lt;link>, &lt;data>, &lt;input>, &lt;img>, and &lt;area> ([#18](https://github.com/delan/autost/issues/18))
- fixed a bug where post titles (.p-name) were sometimes mixed up with author names (.p-author .p-name) ([#18](https://github.com/delan/autost/issues/18))

in `autost server`...
- .mp4 files are now served with the correct mime type
- the **reply** buttons now work correctly on tag pages

thanks to [@LotteMakesStuff](https://github.com/LotteMakesStuff), [@the6p4c](https://github.com/the6p4c), [@VinDuv](https://github.com/VinDuv), and [@Sorixelle](https://github.com/Sorixelle) for their feedback!

# [1.0.0](https://github.com/delan/autost/releases/tag/1.0.0) (2024-10-10)

- check out the new [**autost book**](https://delan.github.io/autost/) for more detailed docs!
- you can now **reply to (or share) posts from other blogs** (`autost import`, `autost reimport`)
  - for consistency, we suggest setting your `[self_author] display_handle` to a domain name
- you can now **create attachments from local files** (`autost attach`)

in the html output…
- **your posts are now rebloggable** thanks to the microformats2 h-entry format (#16)
  - for more details, check out [*Reblogging posts with h-entry*](https://nex-3.com/blog/reblogging-posts-with-h-entry/) by [@nex3](https://github.com/nex3)
- **fragment links** like `[](#section)` now work properly, since we no longer use &lt;base href> (#17)
- author display names are now wrapped in parentheses

in the atom output…
- threads with multiple posts are **no longer an unreadable mess** (#19)
- entries now include **&lt;author>** and **&lt;category>** metadata
- your subscribers will need to convince their clients to redownload your posts, or unsubscribe and resubscribe, to see these effects on existing posts

in `autost server` and the composer…
- added a setting to make `autost server` listen on another port (`server_port`)
- request errors are now more readable, and disappear when requests succeed
- atom feeds (and other .xml files) are now served with the correct mime type
- the **reply** buttons are no longer broken when `base_url` is not `/posts/`
- the **publish** button no longer creates the post file if there are errors

when rendering your site with `autost render` or `autost server`…
- your posts are now rendered on all CPU cores
- we no longer crash if you have no `posts` directory

# [0.3.0](https://github.com/delan/autost/releases/tag/0.3.0) (2024-10-01)

- you can now **download and run autost without building it yourself** or needing the source code
  - this makes the `path_to_autost` setting optional, but you should use `path_to_static` instead
  - `path_to_static` is a new optional setting that lets you replace the stock css and js files
- you can now **click reply on a thread** to share it without typing out the references by hand
- added a command to **create a new autost site** (`autost new`)
- `autost server` now renders your site on startup ([#10](https://github.com/delan/autost/issues/10))
- `autost server` now gives you more details and context when errors occur (thanks [@the6p4c](https://github.com/the6p4c)!)
- `autost render` now generates your `interesting_output_filenames_list_path` in a stable order
- `autost cohost2json` can now run without already having an autost site (autost.toml)
- `autost cohost2autost` now downloads attachments to `attachments`, not `site/attachments`
  - `autost render` instantly copies them from `attachments` to `site/attachments` using hard links
  - `autost render` also updates existing autost sites to move attachments out of `site/attachments`
- removed side margins around threads in narrow viewports
- highlighted the compose button in the web ui

once you’ve built your autost sites with the new `autost render`…
- you can **delete `site`** to render your autost site from scratch! ([#12](https://github.com/delan/autost/issues/12))
- and once you do that, your `site` directory will no longer contain any orphaned attachments ([#11](https://github.com/delan/autost/issues/11))

# [0.2.0](https://github.com/delan/autost/releases/tag/0.2.0) (2024-09-29)

- **breaking change:** `cohost2autost` no longer takes the `./posts` and `site/attachments` arguments
- **breaking change:** `render` no longer takes the `site` argument
  - these paths were always `./posts`, `site/attachments`, and `site` ([#8](https://github.com/delan/autost/issues/8))
- in the `server`, you can now **compose posts** ([#7](https://github.com/delan/autost/issues/7))
- in the `server`, you now get a link to your site in the terminal
- in the `server`, you no longer get a `NotFound` error if your `base_url` setting is not `/`
- you no longer have to type `RUST_LOG=info` to see what’s happening ([#5](https://github.com/delan/autost/issues/5))
- attachment filenames containing `%` and `?` are now handled correctly ([#4](https://github.com/delan/autost/issues/4))

# [0.1.0](https://github.com/delan/autost/releases/tag/0.1.0) (2024-09-27)

initial release (see [announcement](https://cohost.org/delan/post/7848210-autost-a-cohost-com))
