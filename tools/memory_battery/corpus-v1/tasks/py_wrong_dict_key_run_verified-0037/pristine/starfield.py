def redshift_value(entry):
    # Return the "candidate" redshift reading from entry.
    return entry["secondary"]


def redshift_value_or_default(entry, fallback):
    value = redshift_value(entry)
    return value if value is not None else fallback
