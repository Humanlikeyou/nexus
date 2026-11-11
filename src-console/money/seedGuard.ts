/** design-1111t §9.2 Ledger input guard. Runs on every keystroke and paste, BEFORE anything is kept.
 *  Over-refuses on purpose: anything that could be a recovery phrase or private key is cleared, never stored or logged. */
export type GuardResult =
  | { kind: "empty" }
  | { kind: "secret"; why: "phrase" | "private_key" }
  | { kind: "public"; chain: "btc" | "eth"; form: "xpub" | "address"; label: string }
  | { kind: "invalid" };

const B58 = "[1-9A-HJ-NP-Za-km-z]";
const XPRV = new RegExp(`^[xyztuv]prv${B58}{20,}$`);
const WIF = new RegExp(`^[59cKL]${B58}{50,51}$`);
const HEX_KEY = /^(0x)?[0-9a-fA-F]{64}$/;
const XPUB = new RegExp(`^[xyzt]pub${B58}{100,112}$`);
const BTC_LEGACY = new RegExp(`^[13]${B58}{25,34}$`);
const BTC_BECH32 = /^bc1[02-9ac-hj-np-z]{11,87}$/i;
const ETH = /^0x[0-9a-fA-F]{40}$/;
/** A "word": latin letters or Japanese kana (the BIP-39 Japanese list), split on ASCII or ideographic spaces. */
const WORD = /^[a-zA-Z\u3040-\u30ff\u30fc]+$/;

export function guardWatchInput(raw: string): GuardResult {
  const text = raw.trim();
  if (!text) return { kind: "empty" };
  const words = text.split(/[\s\u3000]+/).filter(Boolean);
  // 12, 15, 18, 21 or 24 words is a BIP-39 phrase; refuse anything from 11 words up that is all words, to be safe.
  if (words.length >= 11 && words.every((w) => WORD.test(w.replace(/[.,;:]$/, "")))) return { kind: "secret", why: "phrase" };
  if (words.length === 1) {
    const t = words[0];
    if (XPRV.test(t) || WIF.test(t) || HEX_KEY.test(t)) return { kind: "secret", why: "private_key" };
    if (XPUB.test(t)) return { kind: "public", chain: "btc", form: "xpub", label: "Looks like a public key" };
    if (BTC_BECH32.test(t) || BTC_LEGACY.test(t)) return { kind: "public", chain: "btc", form: "address", label: "Looks like a Bitcoin address" };
    if (ETH.test(t)) return { kind: "public", chain: "eth", form: "address", label: "Looks like an Ethereum address" };
  }
  return { kind: "invalid" };
}
