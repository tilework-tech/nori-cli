/**
 * Get the Rust target triple for the given platform and architecture.
 * @param {string} platform - Node.js process.platform value
 * @param {string} arch - Node.js process.arch value
 * @returns {string|null} - Rust target triple or null if unsupported
 */
export function getTargetTriple(platform, arch) {
  switch (platform) {
    case "android":
      switch (arch) {
        case "x64":
          return "x86_64-linux-android";
        case "arm64":
          return "aarch64-linux-android";
        default:
          return null;
      }
    case "linux":
      switch (arch) {
        case "x64":
          return "x86_64-unknown-linux-musl";
        case "arm64":
          return "aarch64-unknown-linux-musl";
        default:
          return null;
      }
    case "darwin":
      switch (arch) {
        case "x64":
          return "x86_64-apple-darwin";
        case "arm64":
          return "aarch64-apple-darwin";
        default:
          return null;
      }
    case "win32":
      switch (arch) {
        case "x64":
          return "x86_64-pc-windows-msvc";
        case "arm64":
          return "aarch64-pc-windows-msvc";
        default:
          return null;
      }
    default:
      return null;
  }
}
