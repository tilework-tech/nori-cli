# Noridoc: utils/image

Path: @/codex-rs/utils/image

### Overview

The `codex-utils-image` crate provides image loading, resizing, and encoding for Codex. It prepares images for API upload by resizing within bounds and converting to PNG/JPEG with caching.

### How it fits into the larger codebase

Image utils is used for image handling in conversations:

- **Core** uses for processing image attachments
- **TUI** uses for image input handling
- **Prepares** images for model API requirements

### Core Implementation

**Main Function:**

```rust
pub fn load_and_resize_to_fit(path: &Path) -> Result<EncodedImage, ImageProcessingError>
```

Returns `EncodedImage` with bytes, MIME type, and dimensions.

**EncodedImage:**

```rust
pub struct EncodedImage {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub width: u32,
    pub height: u32,
}

impl EncodedImage {
    pub fn into_data_url(self) -> String  // data:mime;base64,...
}
```

### Things to Know

**Size Limits:**

Images are resized to fit within `MAX_WIDTH=2048` and `MAX_HEIGHT=768` using Triangle filter.

**Format Handling:**

- PNG and JPEG pass through if within size limits
- Other formats converted to PNG
- JPEG uses quality 85

**Caching:**

Uses `sha1_digest` of file contents as cache key. LRU cache with capacity 32.

**Async Handling:**

Uses `block_in_place` when reading files inside Tokio runtime.

Created and maintained by Nori.
