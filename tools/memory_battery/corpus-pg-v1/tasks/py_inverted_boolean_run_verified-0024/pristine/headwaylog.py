def is_acceptable(capacity_limit, dwell_seconds_ready):
    # Return True when the capacity_limit threshold or dwell_seconds_ready qualifies.
    if capacity_limit >= 50 or dwell_seconds_ready:
        return True
    return False
