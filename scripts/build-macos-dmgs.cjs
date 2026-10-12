#!/usr/bin/env node
// Builds both macOS DMG variants for APRSWolf and prints the artifact paths.
// Requires Node.js, npm, Rust, and both macOS targets in the active toolchain.

const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const projectRoot = path.resolve(__dirname, "..");
const packageJson = JSON.parse(fs.readFileSync(path.join(projectRoot, "package.json"), "utf8"));
const appVersion = packageJson.version;
const bundleVersion = packageJson.version;
const targets = [
  {
    rustTarget: "x86_64-apple-darwin",
    dmgName: `APRSWolf_${appVersion}_x64.dmg`,
    tauriDmgName: `APRSWolf_${bundleVersion}_x64.dmg`,
  },
  {
    rustTarget: "aarch64-apple-darwin",
    dmgName: `APRSWolf_${appVersion}_aarch64.dmg`,
    tauriDmgName: `APRSWolf_${bundleVersion}_aarch64.dmg`,
  },
];

function run(command) {
  execSync(command, {
    cwd: projectRoot,
    stdio: "inherit",
  });
}

function verifyTargetsInstalled() {
  const installed = execSync("rustup target list --installed", {
    cwd: projectRoot,
    encoding: "utf8",
  })
    .split(/\r?\n/)
    .map((value) => value.trim())
    .filter(Boolean);

  const missing = targets
    .map((target) => target.rustTarget)
    .filter((target) => !installed.includes(target));

  if (missing.length > 0) {
    throw new Error(
      `Missing Rust targets: ${missing.join(", ")}. Install them with rustup target add ${missing.join(" ")}`
    );
  }
}

function formatTimestamp(date) {
  return new Intl.DateTimeFormat("en-US", {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(date);
}

function formatSize(bytes) {
  const mb = bytes / (1024 * 1024);
  return `${mb.toFixed(1)} MB`;
}

function buildTarget(target) {
  console.log(`\n==> Building ${target.rustTarget}...`);
  run(`npm run tauri build -- --target ${target.rustTarget}`);

  const dmgDir = path.join(
    projectRoot,
    "src-tauri",
    "target",
    target.rustTarget,
    "release",
    "bundle",
    "dmg",
  );
  const dmgPath = path.join(dmgDir, target.dmgName);
  const tauriDmgPath = path.join(dmgDir, target.tauriDmgName);

  if (!fs.existsSync(dmgPath) && fs.existsSync(tauriDmgPath)) {
    fs.renameSync(tauriDmgPath, dmgPath);
  }

  if (!fs.existsSync(dmgPath)) {
    throw new Error(`Expected DMG was not created: ${dmgPath}`);
  }

  const stats = fs.statSync(dmgPath);
  return {
    ...target,
    dmgPath,
    size: stats.size,
    modifiedAt: stats.mtime,
  };
}

try {
  verifyTargetsInstalled();

  const artifacts = targets.map(buildTarget);

  console.log("\nmacOS release artifacts:");
  for (const artifact of artifacts) {
    console.log(`- ${artifact.rustTarget}`);
    console.log(`  ${path.relative(projectRoot, artifact.dmgPath)}`);
    console.log(`  ${formatSize(artifact.size)} | ${formatTimestamp(artifact.modifiedAt)}`);
  }
} catch (error) {
  console.error(`\nRelease build failed: ${error.message || String(error)}`);
  process.exit(1);
}
