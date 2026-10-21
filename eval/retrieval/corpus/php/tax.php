---
language: php
---
<?php
// Add the configured sales tax to the listed price at checkout.
function totalWithTax(float $price, float $taxRate): float {
    return round($price + ($price * $taxRate), 2);
}
