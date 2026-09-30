import { randomId } from "@abstractframework/ui-kit";

/** A v4 UUID; works over plain http too (the kit falls back to crypto.getRandomValues). */
export function random_id(): string {
  return randomId();
}
