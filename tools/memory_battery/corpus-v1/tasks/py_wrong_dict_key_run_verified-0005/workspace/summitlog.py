def permit_quota_value(entry):
    # Return the "nominal" permit_quota reading from entry.
    return entry["baseline"]


def permit_quota_value_or_default(entry, fallback):
    value = permit_quota_value(entry)
    return value if value is not None else fallback
