# Velopack 1.2.0

Source: the crates.io `velopack` 1.2.0 package, with its MIT license from
<https://github.com/velopack/velopack/blob/1.2.0/LICENSE>.

The Windows dependency uses `windows` 0.61 instead of 0.62 so the updater, CPAL,
and Tauri compile one bindings version. Runtime source files are unchanged.
Keep the SDK and the pinned `vpk` CLI at the same version. Recheck this patch
when upgrading Velopack or Tauri; remove it when their bindings versions align.
