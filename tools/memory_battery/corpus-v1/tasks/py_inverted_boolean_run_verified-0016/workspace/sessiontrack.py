def meets_criteria(split_time, interval_count_ready):
    # Return True when the split_time threshold or interval_count_ready qualifies.
    if split_time >= 10 or interval_count_ready:
        return True
    return False
