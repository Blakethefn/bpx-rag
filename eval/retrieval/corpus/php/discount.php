---
language: php
---
<?php
// Subtract a promotional discount from the listed amount.
function priceAfterDiscount(float $price, float $discount): float {
    return max(0, round($price - $discount, 2));
}
