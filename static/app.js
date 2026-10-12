// Progressive enhancement only: every admin action also works without JavaScript.

// Ask before submitting destructive forms such as "delete post".
for (const form of document.querySelectorAll("form[data-confirm]")) {
  form.addEventListener("submit", (event) => {
    if (!window.confirm(form.dataset.confirm)) {
      event.preventDefault();
    }
  });
}

// Warn before leaving the editor with unsaved changes.
const editor = document.querySelector("form.editor[action^='/admin/posts']");
if (editor) {
  let dirty = false;
  editor.addEventListener("input", () => {
    dirty = true;
  });
  editor.addEventListener("submit", () => {
    dirty = false;
  });
  window.addEventListener("beforeunload", (event) => {
    if (dirty) {
      event.preventDefault();
    }
  });
}
