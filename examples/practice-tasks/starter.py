"""A deliberately unfinished practice module. Use it only in a scratch folder."""


def delivery_fee(subtotal_cents, express=False):
    """Calculate delivery in integer cents."""
    fee = 0 if subtotal_cents > 5000 else 499
    if express and fee:
        fee += 799
    return fee


def unique_names(names):
    """Return names without duplicates."""
    return sorted(set(names))


def parse_quantities(lines):
    """Read item,quantity lines and sum quantities per item."""
    raise NotImplementedError("Complete this function for practice task 3")
