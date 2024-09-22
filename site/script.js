const compose = document.querySelector("form.compose");
if (compose) {
    const sourceField = compose.querySelector(":scope > textarea.source");
    const previewButton = compose.querySelector(":scope > button.preview");
    const update = async () => {
        const data = new URLSearchParams(new FormData(compose));
        const response = await fetch(compose.action, {
            method: "post",
            body: data,
        });
        const body = await response.text();

        const preview = compose.querySelector(":scope > div.preview");
        preview.innerHTML = body;
    };
    compose.addEventListener("submit", event => {
        event.preventDefault();
        update();
    });
    sourceField.addEventListener("input", event => {
        update();
    });
    previewButton.style.display = "none";
    addEventListener("DOMContentLoaded", event => {
        update();
    });
}
