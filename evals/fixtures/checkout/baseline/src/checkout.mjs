/**
 * @param {number} quantity Positive integer item count.
 * @returns {number} Total cents at a fixed price of 1200 cents per item.
 * @throws {RangeError} When quantity is not a positive integer.
 */
export function checkoutTotal(quantity) {
  if (!Number.isInteger(quantity) || quantity < 1) {
    throw new RangeError('Quantity must be a positive integer');
  }
  return quantity * 1200;
}
