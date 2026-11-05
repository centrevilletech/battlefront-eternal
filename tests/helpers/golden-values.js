// JSON cannot distinguish negative zero or an absent optional scalar. Preserve
// both in the synthetic results recorded before the authority twin was removed.
export function goldenValue(value) {
  if (value?.$number) return value.$number === 'undefined' ? undefined : Number(value.$number);
  if (Array.isArray(value)) return value.map(goldenValue);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([k,v]) => [k,goldenValue(v)]));
  return value;
}
