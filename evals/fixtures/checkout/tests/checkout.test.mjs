import assert from 'node:assert/strict';
import test from 'node:test';
import { checkoutTotal } from '../src/checkout.mjs';

test('calculates positive integer quantities in cents', () => {
  assert.equal(checkoutTotal(1), 1200);
  assert.equal(checkoutTotal(3), 3600);
});

test('rejects negative quantities', () => {
  for (const quantity of [-1, -3]) {
    assert.throws(() => checkoutTotal(quantity), RangeError);
  }
});

test('rejects zero and non-integer quantities', () => {
  for (const quantity of [0, 0.5, -0.5, '1', null, undefined, NaN, Infinity]) {
    assert.throws(() => checkoutTotal(quantity), RangeError);
  }
});
