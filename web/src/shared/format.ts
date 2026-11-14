export function shortRoot(value: string, max: number): string {
  if (value.length <= max) return value;
  return `…${value.slice(value.length - max + 1)}`;
}
