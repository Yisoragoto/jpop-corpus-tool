/** Stable, compact marks: numerical names should never collapse to just “1”. */
export function monogram(name: string): string {
  const value = name.trim();
  if (!value) return "♪";
  const numbers = /^\d{1,3}/u.exec(value);
  if (numbers) return numbers[0];
  const words = value.split(/[\s・]+/u).filter(Boolean);
  if (words.length > 1 && words.every(w => /^[a-z]/iu.test(w))) return words.slice(0, 2).map(w => [...w][0]).join("").toUpperCase();
  return [...value].slice(0, 2).join("").toUpperCase();
}

export function avatarTone(name: string): number {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + ch.codePointAt(0)!) | 0;
  return (hash >>> 0) % 4;
}
