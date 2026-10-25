import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { ru } from "./ru.ts";

const featureRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const CYRILLIC = /[Ѐ-ӿ]/;

function sourceFiles(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      return name === "i18n" ? [] : sourceFiles(path);
    }
    return /\.(ts|tsx)$/.test(name) && !/\.test\./.test(name) ? [path] : [];
  });
}

test("CRM UI copy lives only in i18n/ru.ts (AD-005)", () => {
  const offenders = sourceFiles(featureRoot)
    .filter((path) => CYRILLIC.test(readFileSync(path, "utf8")))
    .map((path) => relative(featureRoot, path));
  assert.deepEqual(offenders, []);
});

test("the dictionary has no empty strings", () => {
  const walk = (value, path) => {
    if (typeof value === "string") {
      assert.ok(value.trim().length > 0, `empty string at ${path}`);
      return;
    }
    for (const [key, child] of Object.entries(value)) {
      walk(child, `${path}.${key}`);
    }
  };
  walk(ru, "ru");
});
