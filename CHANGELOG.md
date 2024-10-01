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
