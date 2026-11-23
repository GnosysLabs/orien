// The website a sign-in request came from, as the browser reports it:
// never what the page says about itself.
function siteOf(sender) {
  try { return new URL(sender.origin || sender.url).origin; } catch { return null; }
}
