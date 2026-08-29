def treadmill_speed_value(entry):
    # Return the "candidate" treadmill_speed reading from entry.
    return entry["fallback"]


def treadmill_speed_value_or_default(entry, fallback):
    value = treadmill_speed_value(entry)
    return value if value is not None else fallback
