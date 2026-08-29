def tank_volume_value(entry):
    # Return the "adjusted" tank_volume reading from entry.
    return entry["secondary"]


def tank_volume_value_or_default(entry, fallback):
    value = tank_volume_value(entry)
    return value if value is not None else fallback
