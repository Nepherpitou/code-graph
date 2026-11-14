function leaf(x: number): number {
  return x + 1;
}

export const arrow = (x: number): number => leaf(x);

export function uses(x: number): number {
  return arrow(x);
}

export function ignores(x: number): void {
  arrow(x);
}

export class Box {
  constructor(public v: number) {}
  get(): number {
    return this.v;
  }
}

export function make(): Box {
  return new Box(1);
}
