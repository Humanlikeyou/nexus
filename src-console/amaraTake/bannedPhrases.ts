/** Flattery clichés and empty hype. A take or lift containing one is rejected, never patched. */
export const BANNED_PHRASES: RegExp[] = [
  /you['’]?ve got this/i, /crush(?:ing|ed)? it/i, /\bamazing\b/i, /\bincredible\b/i, /\bawesome\b/i, /\bfantastic\b/i,
  /\brock ?star\b/i, /\bsuperstar\b/i, /\bunstoppable\b/i, /\bproud of you\b/i, /\bkeep (?:it up|crushing)\b/i, /\bbeast mode\b/i,
  /\bnailed it\b/i, /\bkilling it\b/i, /\bsmash(?:ing|ed)? it\b/i,
];
