def water_capacity_value(entry):
    # Return the "override" water_capacity reading from entry.
    return entry["baseline"]


def water_capacity_value_or_default(entry, fallback):
    value = water_capacity_value(entry)
    return value if value is not None else fallback
