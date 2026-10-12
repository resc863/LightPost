const form = document.querySelector("#post-form");

form?.addEventListener("submit", async (event) => {
  event.preventDefault();
  const payload = Object.fromEntries(new FormData(form));
  payload.status = "draft";
  const response = await fetch("/api/admin/posts", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    alert("저장하지 못했습니다.");
    return;
  }
  alert("초안으로 저장했습니다.");
  form.reset();
});
