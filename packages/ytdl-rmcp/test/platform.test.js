"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const {
  binaryVersion,
  downloadUrl,
  releaseVersion,
  targetFor,
} = require("../lib/platform");
const { binaryVersion: pinnedBinaryVersion } = require("../package.json");

test("maps supported platforms to release assets", () => {
  assert.deepEqual(targetFor("linux", "x64"), {
    asset: "ytdl-rmcp-x86_64.tar.gz",
    binary: "rytdl",
  });
  assert.deepEqual(targetFor("win32", "x64"), {
    asset: "ytdl-rmcp-windows-x86_64.tar.gz",
    binary: "rytdl.exe",
  });
});

test("rejects unsupported platforms", () => {
  assert.throws(() => targetFor("darwin", "arm64"), /Unsupported platform/);
});

test("uses pinned binary version as the binary tag by default", () => {
  assert.equal(binaryVersion(), pinnedBinaryVersion);
  assert.equal(releaseVersion({}), `v${pinnedBinaryVersion}`);
});

test("allows release tag override", () => {
  const env = {
    YTDL_RMCP_BINARY_VERSION: "v9.9.9",
    YTDL_RMCP_RELEASE_BASE_URL: "https://example.test/releases",
  };
  assert.equal(
    downloadUrl(targetFor("linux", "x64"), env),
    "https://example.test/releases/v9.9.9/ytdl-rmcp-x86_64.tar.gz",
  );
});
