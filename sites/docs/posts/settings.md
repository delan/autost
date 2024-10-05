<meta name="title" content="settings (autost.toml)">
<meta name="published" content="2024-10-01T04:30Z">
<link rel="author" href="#" name="autost">
<meta name="author_display_name" content="autost">
<meta name="author_display_handle" content="autost.example">

<dl>
<dt><code>base_url = "/"</code> <small>(required)</small>
<dd>relative url your site will be served under in <code>autost server</code>, or any other web server you deploy it to. must end with a slash.
<dt><code>external_base_url = "https://example.com/"</code> <small>(required)</small>
<dd>absolute url of the web server you are deploying to, for atom output. must end with a slash.
<dt><code>server_port = 8420</code> <small>(optional)</small>
<dd>port to listen on, for <code>autost server</code>.
<dt><code>site_title = "ao!!"</code> <small>(required)</small>
<dd>title of your site as a whole, for both html and atom output.
<dt><code>other_self_authors = ["https://cohost.org/staff"]</code> <small>(required)</small>
<dd>author urls whose posts are considered your own, in addition to <code>[self_author]</code>.
</dl>

the settings below control which posts are considered “interesting” and included in the html and atom output by default. this allows you to curate your imported chosts, and linkify meaningful tags.

<dl>
<dt><code>interesting_tags = [["photography"], ["reading", "watching", "listening"]]</code> <small>(required)</small>
<dd>posts with these tags are considered “interesting” and included by default, regardless of author. these tags also generate tag pages, which are linked to in all of the posts in those tags.

this setting must be a list of lists of tags — the grouping controls how they are displayed in the navigation at the top of the html output.
<dt><code>archived_thread_tags_path = "path/to/archived_thread_tags.txt"</code> <small>(optional)</small>
<dd>path (relative to autost.toml) to a list of additional tags to add to imported posts. you write this, and the format is:
<pre><code># &lt;original url> &lt;tag>,&lt;tag>,...
https://cohost.org/project/post/123456-slug tag,another tag</code></pre>
<dt><code>interesting_output_filenames_list_path = "path/to/output_interesting.txt"</code> <small>(optional)</small>
<dd>path (relative to autost.toml) to a list of paths relative to your <a href="directory-structure.html">site output directory</a>, representing the “interesting” posts and tag pages. <code>autost render</code> writes this, and you need this to use <code>sites/deploy.sh</code>.
<dt><code>interesting_archived_threads_list_path = "path/to/interesting.txt"</code> <small>(optional)</small>
<dd>path (relative to autost.toml) to a list of imported posts that should also be considered “interesting”, regardless of tags or author. you write this, and the format is:
<pre><code># &lt;original url>
https://cohost.org/project/post/123456-slug
https://nex-3.com/blog/reblogging-posts-with-h-entry/</code></pre>
<dt><code>excluded_archived_threads_list_path = "path/to/excluded.txt"</code> <small>(optional)</small>
<dd>path (relative to autost.toml) to a list of imported posts that should <em>not</em> be considered “interesting”, even if your other settings would otherwise consider them interesting. you write this, and the format is:
<pre><code># &lt;original url>
https://cohost.org/project/post/123456-slug</code></pre>
</dl>

use the settings below if you want to tinker with static files like `style.css` and `script.js` without rebuilding your copy of `autost`:

<dl>
<dt><code>path_to_static = "../../static2"</code> <small>(optional)</small>
<dd>path (relative to autost.toml) to a directory with your own versions of the files in <a href="https://github.com/delan/autost/tree/0.3.0/static">autost’s static directory</a>. note that if you set this to the actual static directory in your copy of the source code, <code>autost</code> will still get rebuilt whenever you change any files, which may not be what you want.
<dt><del><code>path_to_static = "../../static2"</code> <small>(optional)</small></del> <small>(deprecated)</small>
<dd>path (relative to autost.toml) to a directory containing a <code>static</code> directory with your own version of the files in <a href="https://github.com/delan/autost/tree/0.3.0/static">autost’s static directory</a>. this doesn’t work as nicely as <code>path_to_static</code>, but it was needed in older versions of autost (&lt; 0.3.0) where static files were not built into the <code>autost</code> binary.
</dl>

# `[self_author]` <span style="font-size: 1rem; font-weight: normal;"><small>(optional)</small></span>

this section is for your details as an author. it has two effects: new posts are prefilled with this author, and posts by this `href` are always considered “interesting”.

<dl>
<dt><code>href = "https://example.com"</code> <small>(required)</small> → <code>&lt;link rel="author" href></code>
<dd>url for <code>&lt;link></code> metadata and your name and handle links. uniquely identifies you for the purposes of checking if a post is your own.
<dt><code>name = "eggbug"</code> <small>(required)</small> → <code>&lt;link rel="author" name></code>
<dd>your name, for atom output.
<dt><code>display_name = "eggbug"</code> <small>(required)</small> → <code>&lt;meta name="author_display_name" content></code>
<dd>your name, for html output.
<dt><code>display_handle = "eggbug"</code> <small>(required)</small> → <code>&lt;meta name="author_display_handle" content></code>
<dd>your handle, for html output. since this is a domain name like <code>example.com</code> in other imported posts (<code>autost import</code>), we recommend setting this to a domain name like <code>example.com</code>, but it can be anything really.
</dl>

# `[renamed_tags]` <span style="font-size: 1rem; font-weight: normal;"><small>(optional)</small></span>

this section is for automatically renaming tags in your posts without editing them. this takes effect *before* `[implied_tags]`.

<dl>
<dt><code>"Laptop stickers" = "laptop stickers"</code>
<dd>renames any occurrence of “Laptop stickers” to “laptop stickers”.
</dl>

# `[implied_tags]` <span style="font-size: 1rem; font-weight: normal;"><small>(optional)</small></span>

this section is for automatically adding tags to your posts when they contain a specific tag. this takes effect *after* `[renamed_tags]`.

you can use this to tag your posts with more general tags (e.g. “photography”) when they have a more specific tag (e.g. “bird photography”). the implied tags (to the right of “`=`”) are inserted *before* the specific tag, so the more general tags come first.

<dl>
<dt><code>"bird photography" = ["birds", "photography"]</code>
<dd>when a post is tagged “bird photography”, replace that tag with “birds”, “photography”, and “bird photography”.
</dl>

# `[[nav]]` <span style="font-size: 1rem; font-weight: normal;"><small>(optional)</small></span>

you can have any number of these sections, or none at all. each of these sections adds a link to the navigation at the top of the html output.

<dl>
<dt><code>href = "."</code>
<dd>url of the link. relative urls are relative to `base_url`, not to the current page.
<dt><code>text = "posts"</code>
<dd>text to display in the link.
</dl>