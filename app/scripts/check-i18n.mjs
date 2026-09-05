#!/usr/bin/env node
/**
 * i18n 一致性校验（Issue #7）：
 * 1. zh-CN 与 en-US 的 key 集合必须完全一致（多出/缺失都报错）。
 * 2. 两份翻译中的 {placeholder} 占位符必须一一对应。
 * 3. 不得出现空翻译。
 * 用法：node scripts/check-i18n.mjs（在前端目录或仓库根均可）
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const localesDir = resolve(here, "../src/i18n/locales");
const files = { "zh-CN": resolve(localesDir, "zh-CN.json"), "en-US": resolve(localesDir, "en-US.json") };

const dicts = {};
for (const [lang, path] of Object.entries(files)) {
  try {
    dicts[lang] = JSON.parse(readFileSync(path, "utf8"));
  } catch (e) {
    console.error(`✗ ${lang} 解析失败: ${path}\n  ${e.message}`);
    process.exit(1);
  }
}

const [zh, en] = [dicts["zh-CN"], dicts["en-US"]];
const zhKeys = new Set(Object.keys(zh));
const enKeys = new Set(Object.keys(en));
const placeholders = (s) => [...String(s).matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

const errors = [];

// 1. key 集合一致
const onlyZh = [...zhKeys].filter((k) => !enKeys.has(k));
const onlyEn = [...enKeys].filter((k) => !zhKeys.has(k));
for (const k of onlyZh) errors.push(`en-US 缺失 key: ${k}`);
for (const k of onlyEn) errors.push(`zh-CN 缺失 key: ${k}`);

// 2. 占位符一致 + 3. 空翻译
for (const k of zhKeys) {
  if (!enKeys.has(k)) continue;
  const a = placeholders(zh[k]);
  const b = placeholders(en[k]);
  if (JSON.stringify(a) !== JSON.stringify(b)) {
    errors.push(`占位符不一致: ${k}  zh=[${a}] en=[${b}]`);
  }
  if (!String(zh[k]).trim() || !String(en[k]).trim()) {
    errors.push(`存在空翻译: ${k}`);
  }
}

if (errors.length) {
  console.error(`✗ i18n 校验失败（${errors.length} 项）：`);
  for (const e of errors) console.error(`  - ${e}`);
  process.exit(1);
}

console.log(`✓ i18n 校验通过：zh-CN / en-US 各 ${zhKeys.size} 个 key，占位符一致，无空翻译。`);
