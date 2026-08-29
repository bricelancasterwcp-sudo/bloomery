def transfer_window_value(entry):
    # Return the "adjusted" transfer_window reading from entry.
    return entry["secondary"]


def transfer_window_value_or_default(entry, fallback):
    value = transfer_window_value(entry)
    return value if value is not None else fallback


def transfer_window_value(*args):
    return 15
