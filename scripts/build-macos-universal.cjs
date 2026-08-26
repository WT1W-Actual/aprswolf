#!/usr/bin/env node
// Builds a universal macOS app and DMG from fresh per-architecture Tauri app bundles.
// The script performs a clean rebuild for both macOS targets before merging them.

const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const projectRoot = path.resolve(__dirname, "..");
const packageJson = JSON.parse(fs.readFileSync(path.join(projectRoot, "package.json"), "utf8"));
function normalizeReleaseVersion(version) {
  if (typeof version !== "string" || version.length === 0) {
    return "0.0.0";
  }

  const semverMatch = version.match(/^(\d+)\.(\d+)\.(\d+)$/);
  if (semverMatch) {
    const [, major, minor, patch] = semverMatch;
    return patch === "0" ? `${Number(major)}.${Number(minor)}` : `${Number(major)}.${Number(minor)}.${Number(patch)}`;
  }

  return version.replace(/\b\d+\b/g, (segment) => String(Number(segment)));
}

function getVersionAliases(packageJson) {
  const aliases = new Set();
  const displayVersion = typeof packageJson.displayVersion === "string" ? packageJson.displayVersion : "";
  const bundleVersion = typeof packageJson.version === "string" ? packageJson.version : "";

  for (const value of [displayVersion, bundleVersion]) {
    if (value) {
      aliases.add(value);
      aliases.add(normalizeReleaseVersion(value));
    }
  }

  return Array.from(aliases).filter(Boolean);
}

const appVersion = normalizeReleaseVersion(packageJson.displayVersion || packageJson.version);
const appName = "APRSWolf";
const archTargets = ["x86_64-apple-darwin", "aarch64-apple-darwin"];

const archBundles = {
  x64: path.join(
    projectRoot,
    "src-tauri",
    "target",
    "x86_64-apple-darwin",
    "release",
    "bundle",
    "macos",
    `${appName}.app`,
  ),
  arm64: path.join(
    projectRoot,
    "src-tauri",
    "target",
    "aarch64-apple-darwin",
    "release",
    "bundle",
    "macos",
    `${appName}.app`,
  ),
};

const stagedBundles = {
  x64: path.join(projectRoot, ".release-staging", "APRSWolf-x64.app"),
  arm64: path.join(projectRoot, ".release-staging", "APRSWolf-arm64.app"),
};

const universalRoot = path.join(
  projectRoot,
  "src-tauri",
  "target",
  "universal-apple-darwin",
  "release",
  "bundle",
);
const universalApp = path.join(universalRoot, "macos", `${appName}.app`);
const universalDmg = path.join(
  universalRoot,
  "dmg",
  `${appName}_${appVersion}_universal.dmg`,
);

function run(command) {
  execSync(command, {
    cwd: projectRoot,
    stdio: "inherit",
  });
}

function runCapture(command) {
  return execSync(command, {
    cwd: projectRoot,
    encoding: "utf8",
  }).trim();
}

function ensureFileExists(filePath, label) {
  if (!fs.existsSync(filePath)) {
    throw new Error(`${label} not found: ${filePath}`);
  }
}

function removeIfExists(targetPath) {
  fs.rmSync(targetPath, { recursive: true, force: true });
}

function mkdirp(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
}

function stageBundle(sourcePath, targetPath, label) {
  ensureFileExists(sourcePath, label);
  mkdirp(path.dirname(targetPath));
  removeIfExists(targetPath);
  run(`ditto "${sourcePath}" "${targetPath}"`);
}

function cleanupVersionAliasDmgs(dmgDir, appName, keepName, aliases) {
  if (!fs.existsSync(dmgDir)) {
    return;
  }

  for (const alias of aliases) {
    const aliasName = `${appName}_${alias}_universal.dmg`;
    if (aliasName === keepName) {
      continue;
    }

    const aliasPath = path.join(dmgDir, aliasName);
    if (fs.existsSync(aliasPath)) {
      fs.rmSync(aliasPath, { force: true });
    }
  }
}

function verifyTargetsInstalled() {
  const installed = execSync("rustup target list --installed", {
    cwd: projectRoot,
    encoding: "utf8",
  })
    .split(/\r?\n/)
    .map((value) => value.trim())
    .filter(Boolean);

  const missing = archTargets.filter((target) => !installed.includes(target));

  if (missing.length > 0) {
    throw new Error(
      `Missing Rust targets: ${missing.join(", ")}. Install them with rustup target add ${missing.join(" ")}`
    );
  }
}

function cleanTargetArtifacts(target) {
  console.log(`\n==> Cleaning ${target} artifacts...`);
  run(`cargo clean --manifest-path src-tauri/Cargo.toml --target ${target}`);
}

function buildTargetApp(target) {
  console.log(`\n==> Building fresh ${target} app bundle...`);
  run(`npm run tauri build -- --bundles app --target ${target}`);
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
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

try {
  verifyTargetsInstalled();
  const buildPlan = [
    {
      target: "x86_64-apple-darwin",
      bundlePath: archBundles.x64,
      stagedPath: stagedBundles.x64,
      label: "Intel app bundle",
    },
    {
      target: "aarch64-apple-darwin",
      bundlePath: archBundles.arm64,
      stagedPath: stagedBundles.arm64,
      label: "Apple Silicon app bundle",
    },
  ];

  for (const entry of buildPlan) {
    cleanTargetArtifacts(entry.target);
    buildTargetApp(entry.target);
    stageBundle(entry.bundlePath, entry.stagedPath, entry.label);
  }

  ensureFileExists(stagedBundles.x64, "Intel staged app bundle");
  ensureFileExists(stagedBundles.arm64, "Apple Silicon staged app bundle");

  const x64Binary = path.join(stagedBundles.x64, "Contents", "MacOS", appName);
  const arm64Binary = path.join(stagedBundles.arm64, "Contents", "MacOS", appName);
  ensureFileExists(x64Binary, "Intel executable");
  ensureFileExists(arm64Binary, "Apple Silicon executable");

  mkdirp(path.dirname(universalApp));
  mkdirp(path.dirname(universalDmg));
  removeIfExists(universalApp);
  removeIfExists(universalDmg);
  cleanupVersionAliasDmgs(
    path.dirname(universalDmg),
    appName,
    path.basename(universalDmg),
    getVersionAliases(packageJson),
  );

  console.log("\n==> Creating universal app bundle...");
  run(`ditto \"${stagedBundles.arm64}\" \"${universalApp}\"`);

  const universalBinary = path.join(universalApp, "Contents", "MacOS", appName);
  removeIfExists(universalBinary);
  run(`lipo -create \"${x64Binary}\" \"${arm64Binary}\" -output \"${universalBinary}\"`);
  run(`chmod +x \"${universalBinary}\"`);

  const archInfo = runCapture(`/usr/bin/file \"${universalBinary}\"`);
  console.log(archInfo);

  console.log("\n==> Creating universal DMG...");
  run(
    `hdiutil create -volname \"${appName}\" -srcfolder \"${universalApp}\" -format UDZO \"${universalDmg}\"`
  );

  const appStats = fs.statSync(universalBinary);
  const dmgStats = fs.statSync(universalDmg);

  console.log("\nUniversal macOS artifacts:");
  console.log(`- app: ${path.relative(projectRoot, universalApp)}`);
  console.log(`  binary: ${formatSize(appStats.size)} | ${formatTimestamp(appStats.mtime)}`);
  console.log(`- dmg: ${path.relative(projectRoot, universalDmg)}`);
  console.log(`  ${formatSize(dmgStats.size)} | ${formatTimestamp(dmgStats.mtime)}`);
  console.log("\nNote: if you sign/notarize releases, re-sign the universal app bundle after lipo.");
} catch (error) {
  console.error(`\nUniversal build failed: ${error.message || String(error)}`);
  process.exit(1);
}
