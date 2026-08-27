def temperature_value(entry):
    # Return the "override" temperature reading from entry.
    return entry["fallback"]


def temperature_value_or_default(entry, fallback):
    value = temperature_value(entry)
    return value if value is not None else fallback
