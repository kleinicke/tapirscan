/** Deeply readonly view of plain result data. */
export type ReadonlyDeep<T> = T extends object
  ? { readonly [K in keyof T]: ReadonlyDeep<T[K]> }
  : T;

/** Freeze plain result data in place, so results stay immutable at runtime. */
export function freeze<T extends object>(value: T): ReadonlyDeep<T> {
  for (const child of Object.values(value))
    if (child !== null && typeof child === "object") freeze(child);
  return Object.freeze(value) as ReadonlyDeep<T>;
}
