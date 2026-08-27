def apparent_magnitude_value(entry):
    # Return the "baseline" apparent_magnitude reading from entry.
    return entry["adjusted"]


def apparent_magnitude_value_or_default(entry, fallback):
    value = apparent_magnitude_value(entry)
    return value if value is not None else fallback
