// Applies the saved theme before first paint, so the page doesn't flash the wrong colors.
try {
  var t = localStorage.getItem("ss-theme");
  if (t === "dark" || t === "light") document.documentElement.dataset.theme = t;
  else if (matchMedia("(prefers-color-scheme: dark)").matches) document.documentElement.dataset.theme = "dark";
} catch (e) {}
