#!/usr/bin/env node
import { builtinModules } from "node:module";
import { existsSync, readFileSync, statSync } from "node:fs";
import { isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const packagePath = process.argv[2];
if (packagePath === undefined || !isAbsolute(packagePath)) {
  throw new Error("usage: check-typescript-browser-entry.mjs <absolute-package-directory>");
}

const packageDirectory = resolve(packagePath);
const metadataPath = resolve(packageDirectory, "package.json");
if (!existsSync(metadataPath) || !statSync(packageDirectory).isDirectory()) {
  throw new Error(`package directory is missing: ${packageDirectory}`);
}

const metadata = JSON.parse(readFileSync(metadataPath, "utf8"));
const browserSubpath = metadata.exports?.["./bars"];
if (
  browserSubpath === null ||
  typeof browserSubpath !== "object" ||
  typeof browserSubpath.types !== "string" ||
  typeof browserSubpath.import !== "string"
) {
  throw new Error("package must expose ./bars with types and import targets");
}
if (!metadata.files?.includes("typescript/dist/bars.*")) {
  throw new Error("npm package files must include the emitted bars entrypoint");
}

function packageFile(target) {
  if (!target.startsWith("./")) throw new Error(`package target must be relative: ${target}`);
  const absolute = resolve(packageDirectory, target);
  const fromPackage = relative(packageDirectory, absolute);
  if (fromPackage.startsWith("..") || isAbsolute(fromPackage)) {
    throw new Error(`package target escapes package directory: ${target}`);
  }
  return absolute;
}

const entryPath = packageFile(browserSubpath.import);
if (!existsSync(entryPath)) throw new Error(`browser entrypoint is missing: ${entryPath}`);
const entryUrl = pathToFileURL(entryPath);
const builtins = new Set(
  builtinModules.flatMap((name) => [name, name.startsWith("node:") ? name : `node:${name}`]),
);
const visited = new Set();
const pending = [entryUrl.href];
const staticImports = [
  /^\s*import\s+[^;]*?\sfrom\s*["']([^"']+)["']\s*;?/gm,
  /^\s*import\s*["']([^"']+)["']\s*;?/gm,
  /^\s*export\s+(?:type\s+)?(?:\*|\{)[^;]*?\sfrom\s*["']([^"']+)["']\s*;?/gm,
];
const dynamicImport = /\bimport\s*\(\s*(["'`])([^"'`]+)\1\s*\)/g;

while (pending.length > 0) {
  const moduleHref = pending.pop();
  if (moduleHref === undefined || visited.has(moduleHref)) continue;
  visited.add(moduleHref);

  const moduleUrl = new URL(moduleHref);
  if (moduleUrl.protocol !== "file:") {
    throw new Error(`browser import graph contains a non-file module: ${moduleHref}`);
  }
  const modulePath = fileURLToPath(moduleUrl);
  const source = readFileSync(modulePath, "utf8");
  if (/\brequire\s*\(/.test(source)) {
    throw new Error(`browser import graph contains CommonJS require(): ${modulePath}`);
  }

  const importSpecifiers = [];
  for (const pattern of staticImports) {
    for (const match of source.matchAll(pattern)) {
      const specifier = match[1];
      if (specifier !== undefined) importSpecifiers.push(specifier);
    }
  }
  const dynamicCalls = [...source.matchAll(/\bimport\s*\(/g)];
  const dynamicSpecifiers = [...source.matchAll(dynamicImport)];
  if (dynamicCalls.length !== dynamicSpecifiers.length) {
    throw new Error(`browser import graph has a non-literal dynamic import: ${modulePath}`);
  }
  for (const match of dynamicSpecifiers) {
    const specifier = match[2];
    if (specifier !== undefined) importSpecifiers.push(specifier);
  }

  for (const specifier of importSpecifiers) {
    if (builtins.has(specifier) || specifier.startsWith("node:")) {
      throw new Error(`browser import graph reaches Node built-in ${specifier} from ${modulePath}`);
    }
    let resolved;
    if (specifier.startsWith("./") || specifier.startsWith("../")) {
      resolved = new URL(specifier, moduleUrl.href).href;
    } else if (specifier === "@bufbuild/protobuf" || specifier.startsWith("@bufbuild/protobuf/")) {
      resolved = import.meta.resolve(specifier);
    } else {
      throw new Error(`browser import graph has an unreviewed external module ${specifier}`);
    }
    if (resolved.startsWith("node:")) {
      throw new Error(`browser import graph resolves ${specifier} to ${resolved}`);
    }
    const resolvedUrl = new URL(resolved);
    if (resolvedUrl.protocol !== "file:") {
      throw new Error(`browser import graph resolves ${specifier} outside local files: ${resolved}`);
    }
    pending.push(resolvedUrl.href);
  }
}

console.log(`browser-conditioned bars import graph contains ${visited.size} local modules and no Node built-ins`);
