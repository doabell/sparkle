# Image search diagnostics — 2026-09-07

These observations were made with read-only requests from the development
machine. They describe the connection tested, not every user's network.

## Shazam

- `GET /services/amapi/v1/catalog/jp/search` returns HTTP 405, HTML, and
  `Error 54113` from a Varnish cache server in HKG. The homepage `/en-us`
  receives the same rejection, so the failure is not confined to artist search.
- Sparkle, desktop Chrome, curl, and legacy Android user agents all receive
  the same result. Adding the client headers used by Shazam libraries also
  leaves it unchanged. The actual Rust provider reproduced HTTP 405.
- This is an edge rejection before JSON parsing. A simple user-agent change
  did not fix it. The available evidence does not distinguish IP reputation,
  regional edge policy, and client/TLS fingerprint filtering, and does not
  establish that the catalog search endpoint was removed.

## DuckDuckGo

- The normal search page and the image search page (`iax=images&ia=images`)
  return HTTP 200 and contain a `vqd` token. The next `i.js` call returns
  HTTP 403 with the short operations-contact message, before JSON parsing.
- Carrying cookies, using a current browser user agent, and sending image-page
  parameters plus browser Accept/Referer headers did not change that result.
- DuckDuckGo's currently served frontend still uses `i.js`. Its URL builder
  now also attaches `jsa`, `jsa_hash`, and `dp`, derived from `window.__sc__`.
  The page contains that verification state. Sparkle's native HTTP request
  implements the older token-only flow. Missing browser verification is a
  concrete compatibility gap and likely explains the rejection; these
  observations alone do not prove an IP ban or exclude additional filtering.
- Primary implementation inspected: the scripts linked by the live
  [DuckDuckGo image search page](https://duckduckgo.com/?q=YOASOBI+artist&iax=images&ia=images),
  specifically `wpm.main.2a933cc841ff5cea45d7.js`, modules 22520 and 14840.
  Treat these script names as dated evidence, not stable API contracts.

Shazam and DuckDuckGo were subsequently removed from Sparkle's provider list
and implementation. Existing local selections were preserved as Custom and
the local provider lists updated in a one-time maintenance operation, with
database backups. This retirement has no migration code in the application.

Deezer now provides artist search and automatic portraits through its public
`https://api.deezer.com/search/artist` endpoint, without a key. Results retain
provider relevance order, prefer the largest available portrait, and skip
empty artist-image placeholders. HTTP failures and API error objects are
reported separately from no matches. The default image order is Custom,
Deezer, Wikipedia. Manually chosen results are saved as Custom. Wikipedia
search and local image selection remain available; Brave is optional with a key.

For a stronger IP-versus-client diagnosis, compare the same URL in a regular
browser on this connection and on another connection. No user's network or
proxy settings were changed during this investigation.
