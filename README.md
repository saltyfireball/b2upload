# B2Upload

A lightweight desktop app for uploading files to Backblaze B2 via the S3-compatible API. Runs on macOS, Windows, and Linux. Built with Rust, Tauri 2, and vanilla JS.

Drop files onto the window, get shareable URLs back instantly.

## Features

- **Drag-and-drop uploads** - drop one or many files onto the window
- **Concurrent uploads** - up to 5 files upload simultaneously with per-file status
- **Two folder modes** - toggle between two independently configured folders (e.g. "private" and "shared")
- **Auto-copy** - single-file uploads are automatically copied to the clipboard
- **Upload history** - browse and copy URLs from previous uploads
- **Encrypted credential storage** - settings are stored in the macOS Keychain via the system keyring
- **Configurable upload paths** - date folders, UUID filenames, overwrite protection, and per-folder URL tokens are all optional

## Settings

Open settings with the gear icon. There are three sections:

### Connection

| Field                  | Description                                                    |
| ---------------------- | -------------------------------------------------------------- |
| **Domain**             | Your public-facing domain (e.g. `media.example.com`)           |
| **Bucket Name**        | The B2 bucket name                                             |
| **S3 Endpoint**        | S3-compatible endpoint (e.g. `s3.us-east-005.backblazeb2.com`) |
| **Application Key ID** | B2 app key ID                                                  |
| **Application Key**    | B2 app key secret                                              |

All five connection fields are required before uploads will work.

### Folders

Two folders can be configured. Each has a name and an optional URL token.

| Field              | Default   | Description                                                                                    |
| ------------------ | --------- | ---------------------------------------------------------------------------------------------- |
| **Folder 1**       | `private` | Name used as the top-level prefix in the object key. Leave blank to upload to the bucket root. |
| **Folder 1 Token** | _(empty)_ | If set, appended as `?token=xxx` to the returned URL. If blank, no token is added.             |
| **Folder 2**       | `shared`  | Same as above for the second folder.                                                           |
| **Folder 2 Token** | _(empty)_ | Same as above.                                                                                 |

The toggle on the main screen switches between Folder 1 and Folder 2. The toggle labels update to match whatever names you've configured (capitalized).

### Upload Options

| Option                | Default | Description                                                                                                                                                                                                       |
| --------------------- | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Date folders**      | On      | Inserts a `YYYY/MM/DD` path segment after the folder name                                                                                                                                                         |
| **UUID filenames**    | On      | Replaces the original filename with a random UUID. Prevents filename collisions.                                                                                                                                  |
| **Overwrite uploads** | Off     | When off and UUID filenames are also off, the app checks if the file already exists before uploading and returns an error if it does. When UUID filenames are on, this check is skipped (no collisions possible). |

### Upload Path Examples

With all defaults and Folder 1 selected:

```text
private/2026/02/20/a3f7c21e-1234-5678-abcd-ef0123456789.png
```

Date folders off:

```text
private/a3f7c21e-1234-5678-abcd-ef0123456789.png
```

UUID filenames off:

```text
private/2026/02/20/screenshot.png
```

Folder name blank, date off, UUID off:

```text
Folder name blank, date off, UUID off:
```

```text
screenshot.png
```

## Tech Stack

- **Backend:** Rust + Tauri 2
- **Frontend:** Vanilla JS + CSS (no build step)
- **Storage:** AWS S3 SDK (Backblaze B2 S3-compatible API)
- **Credentials:** System keyring via `keyring` crate (macOS Keychain, Windows Credential Manager, Linux Secret Service)
- **Async:** Tokio

## macOS Gatekeeper

The app is not currently signed with an Apple Developer certificate. On macOS, you may need to remove the quarantine attribute after installing:

`````sh
xattr -cr /Applications/B2Upload.app
````sh
xattr -cr /Applications/B2Upload.app
`````

## Building

Requires Rust and the Tauri CLI.

```sh
cargo install tauri-cli
cargo tauri dev      # run in development
cargo tauri build    # build .app and .dmg
```

## Project Structure

```text
src/
  index.html        # Single-page UI with all views
  app.js            # Frontend logic
  style.css         # Dark theme styles

src-tauri/
  src/
    main.rs         # Tauri commands and app setup
    storage.rs      # Keyring settings + JSON history
    uploader.rs     # S3 upload logic and path construction
  tauri.conf.json   # App configuration
  Cargo.toml        # Rust dependencies
```
