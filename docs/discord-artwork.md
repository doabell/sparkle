# Discord artwork storage

Configure Artwork storage in **Settings → Discord Rich Presence**: Disabled, Catbox, or S3-compatible storage.

For S3, the endpoint and bucket are required. Set a public URL when serving objects through a CDN or custom domain. Authenticated stores can use an access key, secret key, and optional session token. Region defaults to `us-east-1`, and the object prefix defaults to `sparkle/`. Credentials stay local and are excluded from backups.

When all S3 Settings fields are empty, Sparkle also supports these environment variables:

- `SPARKLE_ARTWORK_S3_ENDPOINT`
- `SPARKLE_ARTWORK_S3_BUCKET`
- `SPARKLE_ARTWORK_S3_PUBLIC_URL`
- `SPARKLE_ARTWORK_S3_ACCESS_KEY`
- `SPARKLE_ARTWORK_S3_SECRET_KEY`
- `SPARKLE_ARTWORK_S3_SESSION_TOKEN`
- `SPARKLE_ARTWORK_S3_REGION`
- `SPARKLE_ARTWORK_S3_PREFIX`

The Settings test action checks list/access permissions and uploads a small test object, which it leaves in the selected store.

In S3 mode, Sparkle lists the configured prefix once, reuses artwork by content hash, and uploads `<hash>.jpg` only when missing. Catbox URLs are preserved exactly. Separate cached URLs for each provider let you switch storage modes without replacing the other provider's filenames or repeating uploads.
