/**
 * Integer money and quantities for the CRM (AD-004), mirroring
 * `crates/buzz-crm/src/money.rs`.
 *
 * Money is kopecks, quantities are thousandths of a unit. Parsing and
 * rounding run on `BigInt`, so no value ever passes through a binary float;
 * `money.test.mjs` replays the relay's `fixtures/money_vectors.json`.
 */

/** Largest accepted money value: 1 000 000 000,00 ₽ in kopecks. */
export const MAX_MONEY_MINOR = 100_000_000_000;
/** Largest accepted quantity: 1 000 000 units in thousandths. */
export const MAX_QTY_MILLI = 1_000_000_000;
/** Thousandths per whole unit. */
export const QTY_SCALE = 1_000;

export type DecimalParseError =
  | "empty"
  | "format"
  | "negative"
  | "precision"
  | "overflow";

export type MoneyParseResult =
  | { ok: true; minor: number }
  | { ok: false; reason: DecimalParseError };

export type QtyParseError = DecimalParseError | "whole";

export type QtyParseResult =
  | { ok: true; milli: number }
  | { ok: false; reason: QtyParseError };

// Spaces (plain, no-break, narrow no-break, thin) are thousands separators.
const GROUP_SEPARATORS = /[\s   ]/g;
const DECIMAL = /^(\d*)(?:[.,](\d*))?$/;
const MINUS = /^[-−]/;

type DecimalResult =
  | { ok: true; scaled: bigint }
  | { ok: false; reason: DecimalParseError };

/**
 * Parse a non-negative decimal typed by a person into an integer scaled by
 * `10^fractionDigits`. Accepts `1 234,50`, `1234.5`, `1234`, `,5`.
 */
function parseScaled(
  text: string,
  fractionDigits: number,
  max: number,
): DecimalResult {
  const compact = text.replace(GROUP_SEPARATORS, "").replace(/₽$/, "");
  if (compact.length === 0) return { ok: false, reason: "empty" };
  if (MINUS.test(compact)) return { ok: false, reason: "negative" };
  const match = DECIMAL.exec(compact);
  const whole = match?.[1] ?? "";
  const fraction = match?.[2] ?? "";
  if (!match || (whole.length === 0 && fraction.length === 0)) {
    return { ok: false, reason: "format" };
  }
  if (fraction.length > fractionDigits) {
    return { ok: false, reason: "precision" };
  }
  const scale = 10n ** BigInt(fractionDigits);
  const scaled =
    BigInt(whole || "0") * scale +
    BigInt(fraction.padEnd(fractionDigits, "0") || "0");
  if (scaled > BigInt(max)) return { ok: false, reason: "overflow" };
  return { ok: true, scaled };
}

/** Parse rubles typed by a person into kopecks. */
export function parseRubles(text: string): MoneyParseResult {
  const result = parseScaled(text, 2, MAX_MONEY_MINOR);
  return result.ok ? { ok: true, minor: Number(result.scaled) } : result;
}

/**
 * Parse a quantity into thousandths. Units sold whole (`piece`,
 * `service_unit`) reject fractions with `whole`.
 */
export function parseQty(
  text: string,
  options: { wholeOnly: boolean },
): QtyParseResult {
  const result = parseScaled(text, 3, MAX_QTY_MILLI);
  if (!result.ok) return result;
  if (options.wholeOnly && result.scaled % BigInt(QTY_SCALE) !== 0n) {
    return { ok: false, reason: "whole" };
  }
  return { ok: true, milli: Number(result.scaled) };
}

function splitScaled(value: number, fractionDigits: number) {
  const scaled = BigInt(value);
  const negative = scaled < 0n;
  const abs = negative ? -scaled : scaled;
  const scale = 10n ** BigInt(fractionDigits);
  return {
    negative,
    whole: abs / scale,
    fraction: (abs % scale).toString().padStart(fractionDigits, "0"),
  };
}

const RUB_FORMAT = new Intl.NumberFormat("ru-RU", {
  style: "currency",
  currency: "RUB",
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

/**
 * `123450` → `1 234,50 ₽` (ru-RU grouping uses no-break spaces). The whole
 * part is formatted from a `BigInt` and the kopecks are spliced in exactly.
 */
export function formatRubles(minor: number): string {
  const { negative, whole, fraction } = splitScaled(minor, 2);
  // Format a non-zero probe so the sign survives for amounts under 1 ₽.
  const probe = whole === 0n ? 1n : whole;
  return RUB_FORMAT.formatToParts(negative ? -probe : probe)
    .map((part) => {
      if (part.type === "fraction") return fraction;
      if (part.type === "integer" && whole === 0n) return "0";
      return part.value;
    })
    .join("");
}

/** Kopecks as an editable field value: `123450` → `1234,50`. */
export function formatRublesInput(minor: number): string {
  const { negative, whole, fraction } = splitScaled(minor, 2);
  return `${negative ? "-" : ""}${whole},${fraction}`;
}

/** Thousandths as an editable field value: `1500` → `1,5`, `5000` → `5`. */
export function formatQty(milli: number): string {
  const { negative, whole, fraction } = splitScaled(milli, 3);
  const trimmed = fraction.replace(/0+$/, "");
  const sign = negative ? "-" : "";
  return trimmed ? `${sign}${whole},${trimmed}` : `${sign}${whole}`;
}

/**
 * `price × qty` in kopecks, rounded half away from zero once per line —
 * identical to `buzz_crm::line_amount`. Throws `RangeError` past the money
 * bound, where the relay would reject the line.
 */
export function lineAmount(priceMinor: number, qtyMilli: number): number {
  const raw = BigInt(priceMinor) * BigInt(qtyMilli);
  const scale = BigInt(QTY_SCALE);
  const half = scale / 2n;
  const rounded = raw >= 0n ? (raw + half) / scale : -((-raw + half) / scale);
  const bound = BigInt(MAX_MONEY_MINOR);
  if (rounded > bound || rounded < -bound) {
    throw new RangeError("line amount exceeds the money bound");
  }
  return Number(rounded);
}

const RUB_COMPACT_FORMAT = new Intl.NumberFormat("ru-RU", {
  notation: "compact",
  maximumFractionDigits: 1,
});

/** Kopecks as a short ru-RU axis label (compact notation, e.g. 1.3 million). */
export function formatRublesCompact(minor: number): string {
  return `${RUB_COMPACT_FORMAT.format(minor / 100)} ₽`;
}
