import { describe, it } from "node:test";
import assert from "node:assert";
import { getTargetTriple } from "./platform.js";

describe("getTargetTriple", () => {
  describe("android platform", () => {
    it("returns aarch64-linux-android for android arm64", () => {
      const result = getTargetTriple("android", "arm64");
      assert.strictEqual(result, "aarch64-linux-android");
    });

    it("returns x86_64-linux-android for android x64", () => {
      const result = getTargetTriple("android", "x64");
      assert.strictEqual(result, "x86_64-linux-android");
    });

    it("returns null for unsupported android arch", () => {
      const result = getTargetTriple("android", "ia32");
      assert.strictEqual(result, null);
    });
  });

  describe("linux platform", () => {
    it("returns x86_64-unknown-linux-musl for linux x64", () => {
      const result = getTargetTriple("linux", "x64");
      assert.strictEqual(result, "x86_64-unknown-linux-musl");
    });

    it("returns aarch64-unknown-linux-musl for linux arm64", () => {
      const result = getTargetTriple("linux", "arm64");
      assert.strictEqual(result, "aarch64-unknown-linux-musl");
    });
  });

  describe("darwin platform", () => {
    it("returns x86_64-apple-darwin for darwin x64", () => {
      const result = getTargetTriple("darwin", "x64");
      assert.strictEqual(result, "x86_64-apple-darwin");
    });

    it("returns aarch64-apple-darwin for darwin arm64", () => {
      const result = getTargetTriple("darwin", "arm64");
      assert.strictEqual(result, "aarch64-apple-darwin");
    });
  });

  describe("win32 platform", () => {
    it("returns x86_64-pc-windows-msvc for win32 x64", () => {
      const result = getTargetTriple("win32", "x64");
      assert.strictEqual(result, "x86_64-pc-windows-msvc");
    });

    it("returns aarch64-pc-windows-msvc for win32 arm64", () => {
      const result = getTargetTriple("win32", "arm64");
      assert.strictEqual(result, "aarch64-pc-windows-msvc");
    });
  });

  describe("unsupported platform", () => {
    it("returns null for unsupported platform", () => {
      const result = getTargetTriple("freebsd", "x64");
      assert.strictEqual(result, null);
    });
  });
});
