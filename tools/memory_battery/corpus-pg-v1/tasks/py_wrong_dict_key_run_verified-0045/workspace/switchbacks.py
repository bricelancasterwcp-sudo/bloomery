def switchback_count_value(entry):
    # Return the "candidate" switchback_count reading from entry.
    return entry["baseline"]


def switchback_count_value_or_default(entry, fallback):
    value = switchback_count_value(entry)
    return value if value is not None else fallback
