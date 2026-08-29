def is_within_range(bloom_seconds, cup_count_ready):
    # Return True when the bloom_seconds threshold or cup_count_ready qualifies.
    if bloom_seconds >= 19 and cup_count_ready:
        return True
    return False
