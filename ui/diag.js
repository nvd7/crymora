/* ponte de diagnóstico — erros do webview vão para o debug.log do Rust
 * (console de release é invisível). Roda antes de qualquer JS da UI.
 * Fica em arquivo próprio para a CSP poder usar script-src 'self'. */
(function () {
  function send(m) {
    try { window.__TAURI_INTERNALS__.invoke("js_diag", { message: m }); }
    catch (e) {}
  }
  window.diag = send;
  window.addEventListener("error", function (e) {
    send("onerror: " + (e.message || "?") + " @" + (e.filename || "?") + ":" + (e.lineno || 0));
  });
  window.addEventListener("unhandledrejection", function (e) {
    var r = e.reason;
    send("rejection: " + (r && r.message ? r.message : String(r)));
  });
  document.addEventListener("DOMContentLoaded", function () { send("boot: UI carregada"); });
})();
