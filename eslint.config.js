// ESLint flat config. Covers all of src-console (TS/TSX app code + the node:test .mjs suites).
import js from "@eslint/js";
import jsxA11y from "eslint-plugin-jsx-a11y";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/**", "src-tauri/**", "node_modules/**", "archive/**"] },
  {
    files: ["src-console/**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended, jsxA11y.flatConfigs.recommended],
    plugins: { "react-hooks": reactHooks },
    languageOptions: { globals: globals.browser },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_", varsIgnorePattern: "^_" }],
      // Settings rows wrap the control and put the text in <div><strong>, one level deeper than the default 2.
      "jsx-a11y/label-has-associated-control": ["error", { depth: 3 }],
    },
  },
  {
    // Moved verbatim from the old src/ tree (never linted there). Behaviour-identical, so these style/compat rules are
    // switched off for this folder only instead of editing code; every other rule (hooks deps, a11y, recommended) still applies.
    files: ["src-console/legacy-core/**/*.{ts,tsx}"],
    rules: {
      "@typescript-eslint/no-unused-vars": "off",
      "@typescript-eslint/no-explicit-any": "off",
      "no-empty": "off",
      "no-misleading-character-class": "off",
      "prefer-const": "off",
      "react-hooks/set-state-in-effect": "off",
    },
  },
  {
    files: ["src-console/**/*.mjs"],
    extends: [js.configs.recommended],
    languageOptions: { globals: { ...globals.node, ...globals.browser }, sourceType: "module" },
  },
);
