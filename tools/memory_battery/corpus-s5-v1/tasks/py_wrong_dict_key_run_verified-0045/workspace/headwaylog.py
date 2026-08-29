def schedule_slot_value(entry):
    # Return the "adjusted" schedule_slot reading from entry.
    return entry["secondary"]


def schedule_slot_value_or_default(entry, fallback):
    value = schedule_slot_value(entry)
    return value if value is not None else fallback
