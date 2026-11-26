import fs from "node:fs";
import path from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";

const YEN = /[¥￥]/;
const SRC = path.resolve(__dirname, "../../src-console");
const files = (dir: string) => fs.readdirSync(path.join(SRC, dir)).filter((f) => /\.(ts|tsx)$/.test(f) && !/\.test\.|fixtures/.test(f)).map((f) => path.join(SRC, dir, f));

/** Every string, template and JSX text literal (comments and regex literals are not strings). */
function literals(file: string): string[] {
  const sf = ts.createSourceFile(file, fs.readFileSync(file, "utf8"), ts.ScriptTarget.ESNext, true, file.endsWith("x") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
  const out: string[] = [];
  const walk = (n: ts.Node) => {
    if (ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n) || ts.isTemplateHead(n) || ts.isTemplateMiddle(n) || ts.isTemplateTail(n) || ts.isJsxText(n)) out.push(n.getText(sf));
    ts.forEachChild(n, walk);
  };
  walk(sf);
  return out;
}

describe("no yen sign anywhere in money or personal", () => {
  it("source strings of money/ and personal/ contain no yen glyph", () => {
    const all = [...files("money"), ...files("personal")];
    expect(all.length).toBeGreaterThan(8);
    const bad = all.flatMap((f) => literals(f).filter((s) => YEN.test(s)).map((s) => `${path.basename(f)}: ${s.slice(0, 60)}`));
    expect(bad).toEqual([]);
  });
});
