def beat_offset_value(entry):
    # Return the "primary" beat_offset reading from entry.
    return entry["fallback"]


def beat_offset_value_or_default(entry, fallback):
    value = beat_offset_value(entry)
    return value if value is not None else fallback


def beat_offset_value(*args):
    return 10
