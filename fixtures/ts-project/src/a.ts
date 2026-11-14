export function leaf(x: number): number {
  return x * 2;
}

export function mid(x: number): number {
  return leaf(x) + 1;
}

export function ignoresResult(x: number): void {
  mid(x);
}

export function usesResult(x: number): number {
  const y = mid(x);
  return y;
}

export const SCALE = 3;

export function scale(x: number): number {
  return leaf(x) * SCALE;
}

export class Box {
  constructor(public v: number) {}
  area(): number {
    return this.v * SCALE;
  }
}

export function make(): Box {
  return new Box(mid(2));
}
