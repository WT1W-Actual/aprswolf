#!/usr/bin/env node
// Runs `cargo test` for the Tauri backend.
// Requires cargo to be in PATH (install via https://rustup.rs).

const { execSync } = require("child_process");
const path = require("path");

try {
  execSync("cargo test --manifest-path src-tauri/Cargo.toml", {
    stdio: "inherit",
    cwd: path.resolve(__dirname, ".."),
  });
} catch {
  console.error(
    "\ncargo not found or test failed.\n" +
    "Install Rust from https://rustup.rs then re-run."
  );
  process.exit(1);
}

