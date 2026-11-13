import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("no audio is captured or written by the chat/voice add path", () => {
  it("the add code has no capture or file-writing API (voice arrives as logged transcript text)", () => {
    for (const f of ["chatAdds.ts", "intent.ts", "useChatAdds.ts"]) {
      const src = readFileSync(`src-console/adds/${f}`, "utf8");
      expect(src).not.toMatch(/getUserMedia|MediaRecorder|AudioContext|writeFile|Blob\(|\.wav|\.webm|\.m4a/);
    }
  });
});
