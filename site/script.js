const form = document.querySelector("form");
if (form) {
    const sourceField = form.querySelector(":scope > textarea.source");
    const previewButton = form.querySelector(":scope > button.preview");
    const update = async () => {
        const data = new URLSearchParams(new FormData(form));
        data.set("bare", "");
        const response = await fetch(form.action, {
            method: "post",
            body: data,
        });
        const body = await response.text();

        const post = document.querySelector("#post");
        const content = post.querySelector(":scope > div.content");
        content.innerHTML = body;
    };
    form.addEventListener("submit", event => {
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
