def algae_growth_value(entry):
    # Return the "secondary" algae_growth reading from entry.
    return entry["override"]


def algae_growth_value_or_default(entry, fallback):
    value = algae_growth_value(entry)
    return value if value is not None else fallback
