export function shortRoot(value: string, max: number): string {
  if (value.length <= max) return value;
  return `…${value.slice(value.length - max + 1)}`;
}

export function fileBase(file: string): string {
  return file.split('/').at(-1) ?? file;
}

const GENERIC_MODULE_STEMS: Record<string, true> = {
  mod: true,
  index: true,
  __init__: true,
  init: true,
};

/** Top-level functions have no class; use their source module as the container label. */
export function moduleLabel(file: string): string {
  const parts = file.split(/[\\/]/).filter(Boolean);
  const base = parts.pop() ?? file;
  const stem = base.replace(/\.[^.]+$/, '');
  if (GENERIC_MODULE_STEMS[stem.toLowerCase()]) {
    return parts.at(-1) ?? stem;
  }
  return stem;
}

export function splitNodeName(qname: string): { container: string; member: string } {
  const split = qname.lastIndexOf('.');
  return split > 0
    ? { container: qname.slice(0, split), member: qname.slice(split + 1) }
    : { container: '', member: qname };
}
