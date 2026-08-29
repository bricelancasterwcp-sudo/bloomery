def cupping_score_value(entry):
    # Return the "baseline" cupping_score reading from entry.
    return entry["primary"]


def cupping_score_value_or_default(entry, fallback):
    value = cupping_score_value(entry)
    return value if value is not None else fallback
