<meta name="title" content="post format">
<meta name="published" content="2024-10-01T03:00Z">
<link rel="author" href="#" name="autost">
<meta name="author_display_name" content="autost">
<meta name="author_display_handle" content="autost.example">

posts are markdown (`.md`) or html (`.html`) fragments with html “front matter” for metadata. the front matter includes…

<dl>
<dt><code>&lt;link rel="archived" href></code>
<dd><a href="https://microformats.org/wiki/index.php?title=existing-rel-values&oldid=70595#HTML5_link_type_extensions">link to the original post</a>, for imported posts.
<dt><code>&lt;link rel="references" href></code>
<dd>one for each post being replied to, including the posts that <em>those</em> posts are replying to (these are <em>not</em> resolved recursively).
<dt><code>&lt;meta name="title" content></code>
<dd>title or “headline” of the post.
<dt><code>&lt;meta name="published" content></code>
<dd>date the post was published, as a <a href="https://datatracker.ietf.org/doc/html/rfc3339#section-5.6">rfc 3339</a> timestamp.
<dt><code>&lt;link rel="author" href name></code>
<dd>author of the post. the <code>name</code> here is used in atom output, while the other author metadata is used in html output.
<dt><code>&lt;meta name="author_display_name" content></code>
<dd>name of the author, used in html output.
<dt><code>&lt;meta name="author_display_handle" content></code>
<dd>handle of the author, used in html output. this is <code>@projectName</code> for chosts (<code>autost cohost2autost</code>), or a domain name like <code>example.com</code> for other imported posts (<code>autost import</code>). we recommend setting this to a domain name like <code>example.com</code>, but it can be anything really.
<dt><code>&lt;meta name="tags" content></code>
<dd>one for each tag associated with the post.
<dt><code>&lt;meta name="is_transparent_share"></code>
<dd>if present, hide the post content area entirely. this is used by <code>autost cohost2autost</code> to make cohost’s “transparent shares” look nicer.
</dl>

see also `templates/post-meta.html` and `PostMeta` internally.
