def telescope_id_value(entry):
    # Return the "primary" telescope_id reading from entry.
    return entry["fallback"]


def telescope_id_value_or_default(entry, fallback):
    value = telescope_id_value(entry)
    return value if value is not None else fallback
