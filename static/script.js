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
    const preview = async () => {
        const response = await submitForm(previewButton.formAction);
        const body = await response.text();

        const preview = compose.querySelector(":scope > div.preview");
        preview.innerHTML = body;
    };
    const publish = async () => {
        const response = await submitForm(publishButton.formAction + "?js");
        const body = await response.text();

        if (response.ok) {
            location = body;
        } else {
            const result = compose.querySelector(":scope > div.result");
            result.textContent = body;
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
