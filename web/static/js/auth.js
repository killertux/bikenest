// CSRF (M2, §18): every htmx request carries the session's synchronizer token
// from the `<meta name="csrf">` tag as the `X-CSRF-Token` header. The token is
// server-side only (per-session); the header middleware validates it on
// state-changing authenticated requests.
document.addEventListener('htmx:configRequest', function (e) {
  var meta = document.querySelector('meta[name="csrf"]');
  if (meta && meta.getAttribute('content')) {
    e.detail.headers['X-CSRF-Token'] = meta.getAttribute('content');
  }
});
