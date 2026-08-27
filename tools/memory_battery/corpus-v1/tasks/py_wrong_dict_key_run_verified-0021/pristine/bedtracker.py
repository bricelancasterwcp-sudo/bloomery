def seedling_count_value(entry):
    # Return the "override" seedling_count reading from entry.
    return entry["adjusted"]


def seedling_count_value_or_default(entry, fallback):
    value = seedling_count_value(entry)
    return value if value is not None else fallback
