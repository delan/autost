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
        const error = compose.querySelector(":scope > div.error");
        if (e instanceof Error) {
            error.textContent = `${e.name}: ${e.message}`;
        } else {
            error.textContent = `${e}`;
        }
    };
    const preview = async () => {
        try {
            const response = await submitForm(previewButton.formAction);
            const body = await response.text();
            if (response.ok) {
                const preview = compose.querySelector(":scope > div.preview");
                preview.innerHTML = body;
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
    const composeUrl = `${document.baseURI}compose`;
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
}
