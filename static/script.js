const compose = document.querySelector("form.compose");
if (compose) {
    const sourceField = compose.querySelector(":scope > textarea.source");
    const previewButton = compose.querySelector(":scope > button.preview");
    const publishButton = compose.querySelector(":scope > button.publish");
    const submitForm = async action => {
        const data = new URLSearchParams(new FormData(compose));
        const response = await fetch(action, {
            method: "post",
            body: data,
        });
        console.debug(`POST ${action}`);
        console.debug(response);
        return response;
    };
    const error = e => {
        const error = compose.querySelector(":scope > pre.error");
        if (e instanceof Error) {
            error.textContent = `${e.name}: ${e.message}`;
        } else {
            error.textContent = `${e}`;
        }
        renderTerminalError(error);
    };
    const preview = async () => {
        try {
            const response = await submitForm(previewButton.formAction);
            const body = await response.text();
            if (response.ok) {
                const preview = compose.querySelector(":scope > div.preview");
                preview.innerHTML = body;
                const error = compose.querySelector(":scope > pre.error");
                error.innerHTML = "";
            } else {
                throw new Error(body);
            }
        } catch (e) {
            error(e);
        }
    };
    const publish = async () => {
        try {
            const response = await submitForm(publishButton.formAction + "?js");
            const body = await response.text();
            if (response.ok) {
                location = body;
                return;
            } else {
                throw new Error(body);
            }
        } catch (e) {
            error(e);
        }
    };
    compose.addEventListener("submit", event => {
        event.preventDefault();
        if (event.submitter.value == "publish") {
            event.submitter.disabled = true;
            publish();
        } else {
            event.preventDefault();
            preview();
        }
    });
    sourceField.addEventListener("input", event => {
        preview();
    });
    previewButton.style.display = "none";
    addEventListener("DOMContentLoaded", event => {
        preview();
    });
}

checkAutostServer();

async function checkAutostServer() {
    // if /compose exists, we are using the autost server.
    const composeUrl = `${document.body.dataset.baseUrl}compose`;
    const composeResponse = await fetch(composeUrl);
    if (!composeResponse.ok) return;

    const navUl = document.querySelector("nav > ul");
    const li = document.createElement("li");
    const a = document.createElement("a");
    a.href = composeUrl;
    a.textContent = "compose";
    a.className = "server";
    li.append(a);
    navUl.append(li);

    for (const thread of document.querySelectorAll("article.thread")) {
        const actions = thread.querySelector(":scope > article.post:last-child > footer > .actions");
        const a = document.createElement("a");
        a.href = `${document.body.dataset.baseUrl}compose?${new URLSearchParams({ reply_to: thread.dataset.originalPath })}`;
        a.textContent = "reply";
        a.className = "server";
        actions.prepend(a);
    }

    for (const tag of document.querySelectorAll("article.post > footer .tags > .tag")) {
        const actions = tag.querySelector(":scope .actions");
        const p_category = tag.querySelector(":scope .p-category");
        const a = document.createElement("a");
        a.href = `${document.body.dataset.baseUrl}compose?${new URLSearchParams({ tags: p_category.textContent })}`;
        a.textContent = "+";
        a.className = "server";
        actions.append(a);
    }
}

function renderTerminalError(pre) {
    // <https://en.wikipedia.org/w/index.php?title=ANSI_escape_code&oldid=1248130213#CSI_(Control_Sequence_Introducer)_sequences>
    const csiRuns = pre.textContent.match(/\x1B\[[\x30-\x3F]*[\x20-\x2F]*[\x40-\x7E]|[^\x1B]+|[^]/g);
    const result = [];
    let fgColor = 0;
    for (const run of csiRuns) {
        const match = run.match(/\x1B\[([\x30-\x3F]*)[\x20-\x2F]*([\x40-\x7E])/);
        if (match) {
            const [, params, mode] = match;
            /* sgr: select graphic rendition */
            if (mode == "m") {
                for (const param of params.split(";")) {
                    const num = parseInt(param || "0", 10);
                    if (`${num}`.length != param.length) {
                        continue;
                    }
                    if (num == 0 || num >= 30 && num <= 37 || num >= 90 && num <= 97) {
                        fgColor = num;
                    }
                }
            }
        } else {
            if (fgColor != 0) {
                const span = document.createElement("span");
                span.style.color = `var(--sgr-${fgColor})`;
                span.append(run);
                result.push(span);
            } else {
                const text = document.createTextNode(run);
                result.push(text);
            }
        }
    }
    pre.innerHTML = "";
    pre.append(...result);
}
