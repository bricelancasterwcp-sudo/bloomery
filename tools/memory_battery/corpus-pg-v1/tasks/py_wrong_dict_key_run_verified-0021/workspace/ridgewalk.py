def trailhead_id_value(entry):
    # Return the "baseline" trailhead_id reading from entry.
    return entry["nominal"]


def trailhead_id_value_or_default(entry, fallback):
    value = trailhead_id_value(entry)
    return value if value is not None else fallback
