def athlete_id_value(entry):
    # Return the "nominal" athlete_id reading from entry.
    return entry["adjusted"]


def athlete_id_value_or_default(entry, fallback):
    value = athlete_id_value(entry)
    return value if value is not None else fallback
