import js from "@eslint/js";
import tseslint from "typescript-eslint";
import svelte from "eslint-plugin-svelte";
import prettier from "eslint-config-prettier";
import globals from "globals";
import path from "node:path";
import { fileURLToPath } from "node:url";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
export default [
  {
    ignores: [
      "**/node_modules/**",
      "**/dist/**",
      "**/build/**",
      "**/target/**",
      "sources/**",
      "datasets/**",
      "core/**",
      "provenance/**",
      "bindings/javascript/src/host.ts",
      "bindings/javascript/src/host64.ts",
      "bindings/javascript/src/multiformat-host.ts",
      "**/experimental-built/**",
      "**/public/**",
    ],
  },
  js.configs.recommended,
  {
    languageOptions: { globals: { ...globals.node, ...globals.browser } },
    rules: {
      "no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_", caughtErrorsIgnorePattern: "^_" },
      ],
    },
  },
  ...tseslint.configs.strictTypeChecked.map((c) => ({
    ...c,
    files: ["**/*.ts", "**/*.mts", "**/*.cts"],
  })),
  {
    files: ["**/*.ts", "**/*.mts", "**/*.cts"],
    languageOptions: {
      parserOptions: { project: path.join(root, "tsconfig.quality.json"), tsconfigRootDir: root },
    },
  },
  {
    files: ["demo/src/**/*.ts"],
    languageOptions: {
      parserOptions: { project: path.join(root, "demo/tsconfig.json"), tsconfigRootDir: root },
    },
  },
  ...svelte.configs["flat/recommended"],
  { files: ["**/*.svelte"], languageOptions: { parserOptions: { parser: tseslint.parser } } },
  prettier,
];
